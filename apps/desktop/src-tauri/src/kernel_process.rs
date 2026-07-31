//! Concrete desktop child-process driver for the native Kernel host.

#![cfg_attr(not(test), allow(dead_code))]

use std::{
    collections::VecDeque,
    future::Future,
    io::BufReader,
    path::{Path, PathBuf},
    pin::Pin,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use qingyu_kernel::{
    contract::ReadyHealthResponse,
    host::native::{NativeHostControl, NativeHostReady},
};
use tokio::sync::oneshot;

use crate::kernel_host::{
    KernelHostFailure, KernelOwnership, KernelProcessFactory, KernelSpawnPermit,
    NativeKernelCredentialLease, NativeKernelLaunch, PendingKernel, ReadyEvidence, RunningKernel,
    SynchronousKernelGuard,
};

const STDERR_TAIL_LIMIT: usize = 8 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);
const READY_RETRY_INTERVAL: Duration = Duration::from_millis(25);

pub(crate) struct NativeKernelProcessFactory {
    executable: PathBuf,
    http: reqwest::Client,
}

impl NativeKernelProcessFactory {
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn for_current_application() -> Result<Self, KernelHostFailure> {
        let current = std::env::current_exe().map_err(|_| KernelHostFailure::Spawn)?;
        let executable = sidecar_path_for_executable(&current, std::env::consts::EXE_SUFFIX)
            .ok_or(KernelHostFailure::Spawn)?;
        Self::new(executable)
    }

    fn new(executable: PathBuf) -> Result<Self, KernelHostFailure> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| KernelHostFailure::Spawn)?;
        Ok(Self { executable, http })
    }
}

impl KernelProcessFactory for NativeKernelProcessFactory {
    fn spawn(
        &self,
        launch: NativeKernelLaunch,
        permit: KernelSpawnPermit,
        ownership: &KernelOwnership,
    ) -> Result<Box<dyn PendingKernel>, KernelHostFailure> {
        let generation = permit.into_generation();
        let permit = ownership.begin_spawn(generation)?;
        let (startup, credential) = launch.into_parts();
        let mut child = Command::new(&self.executable)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| KernelHostFailure::Spawn)?;
        let stdin = child.stdin.take().ok_or_else(|| {
            terminate_unregistered_child(&mut child);
            KernelHostFailure::Spawn
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            terminate_unregistered_child(&mut child);
            KernelHostFailure::Spawn
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            terminate_unregistered_child(&mut child);
            KernelHostFailure::Spawn
        })?;
        let process = Arc::new(ProcessGuard {
            state: Mutex::new(ProcessState {
                child,
                stdin: Some(stdin),
                reaped: false,
            }),
        });

        if write_startup(&process, &startup).is_err() {
            process.terminate_and_reap_or_abort();
            credential.revoke();
            return Err(KernelHostFailure::Spawn);
        }

        let (ready_sender, ready_receiver) = oneshot::channel();
        if std::thread::Builder::new()
            .name("qingyu-kernel-ready".to_owned())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let ready = NativeHostReady::read_json_line(&mut reader)
                    .map_err(|_| KernelHostFailure::Protocol);
                let _send_result = ready_sender.send(ready);
            })
            .is_err()
        {
            process.terminate_and_reap_or_abort();
            credential.revoke();
            return Err(KernelHostFailure::Spawn);
        }

        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_LIMIT)));
        let stderr_tail_writer = Arc::clone(&stderr_tail);
        if std::thread::Builder::new()
            .name("qingyu-kernel-stderr".to_owned())
            .spawn(move || drain_stderr(stderr, &stderr_tail_writer))
            .is_err()
        {
            process.terminate_and_reap_or_abort();
            credential.revoke();
            return Err(KernelHostFailure::Spawn);
        }

        permit.register(Arc::clone(&process) as Arc<dyn SynchronousKernelGuard>);
        Ok(Box::new(NativePendingKernel {
            process,
            ready_receiver: Some(ready_receiver),
            credential,
            http: self.http.clone(),
            _stderr_tail: stderr_tail,
            armed: true,
        }))
    }
}

struct NativePendingKernel {
    process: Arc<ProcessGuard>,
    ready_receiver: Option<oneshot::Receiver<Result<NativeHostReady, KernelHostFailure>>>,
    credential: NativeKernelCredentialLease,
    http: reqwest::Client,
    _stderr_tail: Arc<Mutex<VecDeque<u8>>>,
    armed: bool,
}

impl PendingKernel for NativePendingKernel {
    fn wait_ready(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<ReadyEvidence, KernelHostFailure>> + Send + '_>> {
        Box::pin(async move {
            let receiver = self
                .ready_receiver
                .take()
                .ok_or(KernelHostFailure::Protocol)?;
            let ready = receiver.await.map_err(|_| KernelHostFailure::EarlyExit)??;
            let authenticated_instance = loop {
                if let Some(_status) = self.process.try_wait()? {
                    return Err(KernelHostFailure::EarlyExit);
                }
                let url = format!("http://127.0.0.1:{}/api/v1/health/ready", ready.port());
                let request = self
                    .credential
                    .with_secret(|secret| self.http.get(&url).bearer_auth(secret).send())?;
                match request.await {
                    Ok(response) if response.status().is_success() => {
                        let health = response
                            .json::<ReadyHealthResponse>()
                            .await
                            .map_err(|_| KernelHostFailure::Protocol)?;
                        break health.instance_id;
                    }
                    Ok(_response) => return Err(KernelHostFailure::Protocol),
                    Err(_error) => tokio::time::sleep(READY_RETRY_INTERVAL).await,
                }
            };
            Ok(ReadyEvidence {
                ready,
                authenticated_instance,
            })
        })
    }

    fn credential_lease(&self) -> NativeKernelCredentialLease {
        self.credential.clone()
    }

    fn cancel_and_reap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
        Box::pin(async move {
            self.process.request_shutdown()?;
            self.process.wait_reaped().await?;
            self.disarm();
            Ok(())
        })
    }

    fn force_kill_and_reap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
        let process = Arc::clone(&self.process);
        let reap = process.spawn_force_kill_and_reap();
        self.disarm();
        Box::pin(async move { reap.await.map_err(|_| KernelHostFailure::StopFailed)? })
    }

    fn into_running(mut self: Box<Self>) -> Result<Box<dyn RunningKernel>, KernelHostFailure> {
        if self.process.try_wait()?.is_some() {
            return Err(KernelHostFailure::EarlyExit);
        }
        self.armed = false;
        Ok(Box::new(NativeRunningKernel {
            process: Arc::clone(&self.process),
            credential: self.credential.clone(),
            _stderr_tail: Arc::clone(&self._stderr_tail),
            armed: true,
        }))
    }
}

impl NativePendingKernel {
    fn disarm(&mut self) {
        self.credential.revoke();
        self.armed = false;
    }
}

impl Drop for NativePendingKernel {
    fn drop(&mut self) {
        if self.armed {
            self.credential.revoke();
            self.process.terminate_and_reap_or_abort();
        }
    }
}

struct NativeRunningKernel {
    process: Arc<ProcessGuard>,
    credential: NativeKernelCredentialLease,
    _stderr_tail: Arc<Mutex<VecDeque<u8>>>,
    armed: bool,
}

impl RunningKernel for NativeRunningKernel {
    fn wait_exit(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
        Box::pin(async move {
            self.process.wait_reaped().await?;
            self.disarm();
            Ok(())
        })
    }

    fn shutdown_and_reap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
        Box::pin(async move {
            self.process.request_shutdown()?;
            self.process.wait_reaped().await?;
            self.disarm();
            Ok(())
        })
    }

    fn force_kill_and_reap(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), KernelHostFailure>> + Send + '_>> {
        let process = Arc::clone(&self.process);
        let reap = process.spawn_force_kill_and_reap();
        self.disarm();
        Box::pin(async move { reap.await.map_err(|_| KernelHostFailure::StopFailed)? })
    }
}

impl NativeRunningKernel {
    fn disarm(&mut self) {
        self.credential.revoke();
        self.armed = false;
    }
}

impl Drop for NativeRunningKernel {
    fn drop(&mut self) {
        if self.armed {
            self.credential.revoke();
            self.process.terminate_and_reap_or_abort();
        }
    }
}

struct ProcessGuard {
    state: Mutex<ProcessState>,
}

struct ProcessState {
    child: Child,
    stdin: Option<ChildStdin>,
    reaped: bool,
}

impl ProcessGuard {
    fn request_shutdown(&self) -> Result<(), KernelHostFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| KernelHostFailure::StopFailed)?;
        if state.reaped {
            return Ok(());
        }
        let mut stdin = state.stdin.take().ok_or(KernelHostFailure::StopFailed)?;
        NativeHostControl::write_shutdown_json_line(&mut stdin)
            .map_err(|_| KernelHostFailure::StopFailed)
    }

    fn try_wait(&self) -> Result<Option<std::process::ExitStatus>, KernelHostFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| KernelHostFailure::UnexpectedExit)?;
        if state.reaped {
            return Err(KernelHostFailure::UnexpectedExit);
        }
        let status = state
            .child
            .try_wait()
            .map_err(|_| KernelHostFailure::UnexpectedExit)?;
        if status.is_some() {
            state.reaped = true;
            state.stdin.take();
        }
        Ok(status)
    }

    async fn wait_reaped(&self) -> Result<(), KernelHostFailure> {
        loop {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            tokio::time::sleep(PROCESS_POLL_INTERVAL).await;
        }
    }

    fn spawn_force_kill_and_reap(
        self: Arc<Self>,
    ) -> tokio::task::JoinHandle<Result<(), KernelHostFailure>> {
        tokio::task::spawn_blocking(move || self.force_kill_and_reap_blocking())
    }

    fn force_kill_and_reap_blocking(&self) -> Result<(), KernelHostFailure> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| KernelHostFailure::StopFailed)?;
        if state.reaped {
            return Ok(());
        }
        state.stdin.take();
        match state.child.try_wait() {
            Ok(Some(_status)) => {
                state.reaped = true;
                Ok(())
            }
            Ok(None) => {
                state
                    .child
                    .kill()
                    .map_err(|_| KernelHostFailure::StopFailed)?;
                state
                    .child
                    .wait()
                    .map_err(|_| KernelHostFailure::StopFailed)?;
                state.reaped = true;
                Ok(())
            }
            Err(_error) => Err(KernelHostFailure::StopFailed),
        }
    }
}

impl SynchronousKernelGuard for ProcessGuard {
    fn terminate_and_reap_or_abort(&self) {
        if self.force_kill_and_reap_blocking().is_err() {
            std::process::abort();
        }
    }
}

fn write_startup(
    process: &ProcessGuard,
    startup: &qingyu_kernel::host::native::NativeHostStart,
) -> Result<(), KernelHostFailure> {
    let mut state = process.state.lock().map_err(|_| KernelHostFailure::Spawn)?;
    let stdin = state.stdin.as_mut().ok_or(KernelHostFailure::Spawn)?;
    startup
        .write_json_line(stdin)
        .map_err(|_| KernelHostFailure::Spawn)
}

fn terminate_unregistered_child(child: &mut Child) {
    let _kill_result = child.kill();
    if child.wait().is_err() {
        std::process::abort();
    }
}

fn drain_stderr(mut stderr: impl std::io::Read, tail: &Mutex<VecDeque<u8>>) {
    let mut buffer = [0_u8; 1024];
    loop {
        let read = match stderr.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let Ok(mut tail) = tail.lock() else {
            return;
        };
        for byte in &buffer[..read] {
            if tail.len() == STDERR_TAIL_LIMIT {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }
}

fn sidecar_path_for_executable(executable: &Path, executable_suffix: &str) -> Option<PathBuf> {
    Some(
        executable
            .parent()?
            .join(format!("qingyu-kernel{executable_suffix}")),
    )
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        path::Path,
        process::{Command, Stdio},
        sync::{mpsc, Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };

    use qingyu_kernel::host::native::NativeHostWorkspaceState;

    use super::{
        sidecar_path_for_executable, NativeKernelProcessFactory, NativeRunningKernel, ProcessGuard,
        ProcessState,
    };
    use crate::{
        kernel_host::{
            KernelHostSupervisor, KernelHostTimeouts, NativeKernelLaunch, RunningKernel,
        },
        writer_authority::{KernelWriterPublicationGate, WorkspaceRootIdentity, WriterAuthority},
    };

    #[test]
    #[ignore = "test-only child process for force-reap timing tests"]
    fn blocking_child_process_for_reap_test() {
        thread::sleep(Duration::from_secs(60));
    }

    fn test_running_kernel() -> (NativeRunningKernel, Arc<ProcessGuard>) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--ignored")
            .arg("--exact")
            .arg("kernel_process::tests::blocking_child_process_for_reap_test")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(child.try_wait().unwrap(), None);

        let process = Arc::new(ProcessGuard {
            state: Mutex::new(ProcessState {
                child,
                stdin: None,
                reaped: false,
            }),
        });
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let (_, credential) = NativeKernelLaunch::desktop(
            workspace.clone(),
            app_data,
            cache,
            NativeHostWorkspaceState::for_workspace(&workspace, "Workspace").unwrap(),
            "tauri://localhost".to_owned(),
        )
        .unwrap()
        .into_parts();
        let running = NativeRunningKernel {
            process: Arc::clone(&process),
            credential,
            _stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
            armed: true,
        };
        (running, process)
    }

    fn hold_process_state(
        process: &Arc<ProcessGuard>,
        duration: Duration,
    ) -> thread::JoinHandle<()> {
        let process = Arc::clone(process);
        let (acquired_sender, acquired_receiver) = mpsc::sync_channel(0);
        let holder = thread::spawn(move || {
            let _state = process.state.lock().unwrap();
            acquired_sender.send(()).unwrap();
            thread::sleep(duration);
        });
        acquired_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        holder
    }

    #[tokio::test(flavor = "current_thread")]
    async fn force_reap_keeps_the_async_runtime_responsive_while_process_wait_is_blocked() {
        let (mut running, process) = test_running_kernel();
        let holder = hold_process_state(&process, Duration::from_millis(400));
        let started = Instant::now();

        let heartbeat = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            started.elapsed()
        };
        let force_reap = running.force_kill_and_reap();
        let (force_result, heartbeat_elapsed) = tokio::join!(force_reap, heartbeat);

        assert_eq!(force_result, Ok(()));
        assert!(
            heartbeat_elapsed < Duration::from_millis(200),
            "force reap blocked the async runtime for {heartbeat_elapsed:?}"
        );
        holder.join().unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn timed_out_force_reap_does_not_run_a_second_blocking_reap_from_drop() {
        let (mut running, process) = test_running_kernel();
        let holder = hold_process_state(&process, Duration::from_millis(400));
        let started = Instant::now();

        let result =
            tokio::time::timeout(Duration::from_millis(40), running.force_kill_and_reap()).await;
        assert!(result.is_err());
        drop(running);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "dropping a timed-out force reap blocked for {:?}",
            started.elapsed()
        );

        holder.join().unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if process.state.lock().unwrap().reaped {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unpolled_force_reap_future_still_owns_and_reaps_the_child() {
        let (mut running, process) = test_running_kernel();
        let holder = hold_process_state(&process, Duration::from_millis(400));
        let started = Instant::now();

        let force_reap = running.force_kill_and_reap();
        drop(force_reap);
        drop(running);
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "dropping an unpolled force reap blocked for {:?}",
            started.elapsed()
        );

        holder.join().unwrap();
        let reaped = tokio::time::timeout(Duration::from_millis(200), async {
            loop {
                if process.state.lock().unwrap().reaped {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        if !reaped {
            process.force_kill_and_reap_blocking().unwrap();
        }
        assert!(reaped, "an unpolled force-reap future abandoned the child");
    }

    #[test]
    fn sidecar_is_resolved_beside_the_application_executable() {
        assert_eq!(
            sidecar_path_for_executable(
                Path::new("/Applications/QingYu.app/Contents/MacOS/qingyu"),
                ""
            ),
            Some(Path::new("/Applications/QingYu.app/Contents/MacOS/qingyu-kernel").to_owned())
        );
    }

    #[test]
    fn sidecar_uses_the_platform_executable_suffix() {
        assert_eq!(
            sidecar_path_for_executable(Path::new("/opt/qingyu/qingyu.exe"), ".exe"),
            Some(Path::new("/opt/qingyu/qingyu-kernel.exe").to_owned())
        );
    }

    #[tokio::test]
    #[ignore = "requires QINGYU_KERNEL_TEST_BINARY"]
    async fn native_child_reaches_authenticated_readiness_and_is_reaped() {
        let executable = std::env::var_os("QINGYU_KERNEL_TEST_BINARY")
            .map(std::path::PathBuf::from)
            .expect("QINGYU_KERNEL_TEST_BINARY must identify the built Kernel executable");
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        let cache = temporary.path().join("cache");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let writer_root = WorkspaceRootIdentity::open(&workspace).unwrap();
        let writer_gate = KernelWriterPublicationGate::new(
            WriterAuthority::new(writer_root.clone()),
            writer_root,
        )
        .unwrap();
        let launch = NativeKernelLaunch::desktop(
            workspace.clone(),
            app_data,
            cache,
            NativeHostWorkspaceState::for_workspace(&workspace, "Workspace").unwrap(),
            "tauri://localhost".to_owned(),
        )
        .unwrap();
        let factory = NativeKernelProcessFactory::new(executable).unwrap();
        let supervisor = KernelHostSupervisor::new(
            Arc::new(factory),
            KernelHostTimeouts::uniform(Duration::from_secs(10)),
            writer_gate,
        );

        let access = supervisor.start(launch).await.unwrap();
        assert!(access.endpoint.port > 0);
        assert!(access.credential.is_available());

        supervisor.stop().await.unwrap();
        assert!(!access.credential.is_available());
    }
}
