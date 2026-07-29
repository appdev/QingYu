use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::TcpStream,
    process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tempfile::tempdir;

use qingyu_kernel::host::native::{
    NativeHostControl, NativeHostReady, NativeHostWorkspaceState, NATIVE_HOST_PROTOCOL_VERSION,
};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const VALID_CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[test]
fn desktop_startup_reports_public_readiness_and_serves_live_probe() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();

    let startup = json!({
        "type": "start",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "profile": "desktop",
        "workspaceRoot": workspace,
        "appDataRoot": app_data,
        "cacheRoot": cache,
        "workspaceState": workspace_state(&workspace),
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness_line = process.read_stdout_line(PROCESS_TIMEOUT);
    let readiness: Value = serde_json::from_str(readiness_line.trim()).unwrap();
    let parsed_readiness =
        NativeHostReady::read_json_line(&mut BufReader::new(readiness_line.as_bytes())).unwrap();

    assert_eq!(
        readiness.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["instanceId", "port", "protocolVersion", "type"]
    );
    assert_eq!(readiness["type"], "ready");
    assert_eq!(readiness["protocolVersion"], NATIVE_HOST_PROTOCOL_VERSION);
    assert!(readiness["instanceId"].as_str().is_some());
    assert!(!readiness_line.contains(VALID_CREDENTIAL));

    let port = readiness["port"].as_u64().unwrap();
    assert_eq!(parsed_readiness.port(), u16::try_from(port).unwrap());
    assert_eq!(
        serde_json::to_value(parsed_readiness.instance_id()).unwrap(),
        readiness["instanceId"]
    );
    let response = live_probe(u16::try_from(port).unwrap());
    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected live response: {response}"
    );
    assert!(response.contains(r#""status":"live""#));

    let ready_response = authorized_get(u16::try_from(port).unwrap(), "/api/v1/health/ready");
    assert!(
        ready_response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected ready response: {ready_response}"
    );
    let ready_body: Value = serde_json::from_str(response_body(&ready_response)).unwrap();
    assert_eq!(ready_body["instanceId"], readiness["instanceId"]);
    assert_eq!(process.child_mut().try_wait().unwrap(), None);
}

#[test]
fn standalone_process_installs_durable_settings_and_reports_the_capability() {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();

    let startup = json!({
        "type": "start",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "profile": "desktop",
        "workspaceRoot": workspace,
        "appDataRoot": app_data,
        "cacheRoot": cache,
        "workspaceState": workspace_state(&workspace),
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness: Value =
        serde_json::from_str(process.read_stdout_line(PROCESS_TIMEOUT).trim()).unwrap();
    let port = u16::try_from(readiness["port"].as_u64().unwrap()).unwrap();

    let runtime = authorized_get(port, "/api/v1/runtime");
    assert!(runtime.starts_with("HTTP/1.1 200 OK\r\n"), "{runtime}");
    let runtime_body: Value = serde_json::from_str(response_body(&runtime)).unwrap();
    assert_eq!(runtime_body["capabilities"]["settings"], true);
    assert_eq!(runtime_body["capabilities"]["portableSettings"], true);

    let settings = authorized_get(port, "/api/v1/settings");
    assert!(settings.starts_with("HTTP/1.1 200 OK\r\n"), "{settings}");
    let settings_body: Value = serde_json::from_str(response_body(&settings)).unwrap();
    assert!(settings_body["revision"].as_str().is_some());
    assert!(app_data.join("settings.json").is_file());
    assert_eq!(process.child_mut().try_wait().unwrap(), None);
}

#[test]
fn standalone_process_installs_the_host_committed_workspace_across_restarts() {
    let (startup, root) = desktop_startup_fixture();
    let mut first = KernelProcess::spawn(&startup.to_string());
    let first_readiness: Value =
        serde_json::from_str(first.read_stdout_line(PROCESS_TIMEOUT).trim()).unwrap();
    let first_port = u16::try_from(first_readiness["port"].as_u64().unwrap()).unwrap();
    let first_response = authorized_get(first_port, "/api/v1/workspace");
    assert!(
        first_response.starts_with("HTTP/1.1 200 OK\r\n"),
        "{first_response}"
    );
    let first_workspace: Value = serde_json::from_str(response_body(&first_response)).unwrap();
    assert_eq!(first_workspace["displayName"], "Notes");
    let revision_seed = startup["workspaceState"]["primaryWorkspace"]["revisionSeed"]
        .as_str()
        .unwrap()
        .as_bytes();
    for entry in std::fs::read_dir(root.path().join("app-data")).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            assert!(
                !bytes
                    .windows(revision_seed.len())
                    .any(|window| window == revision_seed),
                "the child must not create a second durable workspace authority"
            );
        }
    }
    first.write_shutdown();
    assert!(first.wait_for_output(PROCESS_TIMEOUT).status.success());

    let mut second = KernelProcess::spawn(&startup.to_string());
    let second_readiness: Value =
        serde_json::from_str(second.read_stdout_line(PROCESS_TIMEOUT).trim()).unwrap();
    let second_port = u16::try_from(second_readiness["port"].as_u64().unwrap()).unwrap();
    let second_response = authorized_get(second_port, "/api/v1/workspace");
    assert!(
        second_response.starts_with("HTTP/1.1 200 OK\r\n"),
        "{second_response}"
    );
    let second_workspace: Value = serde_json::from_str(response_body(&second_response)).unwrap();

    assert_ne!(
        first_readiness["instanceId"],
        second_readiness["instanceId"]
    );
    assert_eq!(first_workspace, second_workspace);
}

#[test]
fn composition_failure_after_lock_acquisition_releases_roots_before_exit() {
    let (startup, root) = desktop_startup_fixture();
    let private_marker = "private-invalid-settings-marker";
    std::fs::write(root.path().join("app-data/settings.json"), private_marker).unwrap();

    let failed = KernelProcess::spawn(&startup.to_string()).wait_for_output(PROCESS_TIMEOUT);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(utf8(&failed.stderr).trim(), "QingYu Kernel startup failed.");
    assert!(!utf8(&failed.stderr).contains(private_marker));

    std::fs::remove_file(root.path().join("app-data/settings.json")).unwrap();
    let mut restarted = KernelProcess::spawn(&startup.to_string());
    let readiness = restarted.read_stdout_line(PROCESS_TIMEOUT);
    assert!(readiness.contains(r#""type":"ready""#));
}

#[test]
fn committed_workspace_state_cannot_be_reused_for_another_physical_root() {
    let root = tempdir().unwrap();
    let committed_workspace = root.path().join("committed-workspace");
    let other_workspace = root.path().join("other-workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    for path in [&committed_workspace, &other_workspace, &app_data, &cache] {
        std::fs::create_dir(path).unwrap();
    }
    let workspace_state = workspace_state(&committed_workspace);
    let mismatched = json!({
        "type": "start",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "profile": "desktop",
        "workspaceRoot": other_workspace,
        "appDataRoot": app_data,
        "cacheRoot": cache,
        "workspaceState": workspace_state,
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });

    let rejected = KernelProcess::spawn(&mismatched.to_string()).wait_for_output(PROCESS_TIMEOUT);
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    assert_eq!(
        utf8(&rejected.stderr).trim(),
        "QingYu Kernel startup failed."
    );

    let mut matched = mismatched;
    matched["workspaceRoot"] = json!(committed_workspace);
    let mut restarted = KernelProcess::spawn(&matched.to_string());
    assert!(restarted
        .read_stdout_line(PROCESS_TIMEOUT)
        .contains(r#""type":"ready""#));
}

#[test]
fn explicit_shutdown_frame_stops_the_process_cleanly_without_extra_stdout() {
    let (startup, _root) = desktop_startup_fixture();
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness = process.read_stdout_line(PROCESS_TIMEOUT);
    assert!(readiness.contains(r#""type":"ready""#));

    process.write_shutdown();
    let output = process.wait_for_output(PROCESS_TIMEOUT);

    assert!(output.status.success(), "stderr: {}", utf8(&output.stderr));
    assert_eq!(utf8(&output.stdout).lines().count(), 1);
    assert!(output.stderr.is_empty());
}

#[test]
fn closing_the_control_lease_stops_the_process_cleanly() {
    let (startup, _root) = desktop_startup_fixture();
    let mut process = KernelProcess::spawn(&startup.to_string());
    process.read_stdout_line(PROCESS_TIMEOUT);

    process.close_control();
    let output = process.wait_for_output(PROCESS_TIMEOUT);

    assert!(output.status.success(), "stderr: {}", utf8(&output.stderr));
    assert_eq!(utf8(&output.stdout).lines().count(), 1);
    assert!(output.stderr.is_empty());
}

#[test]
fn a_second_start_frame_is_rejected_as_control_without_disclosing_startup_data() {
    let secret_path_marker = "kernel-protocol-private-path-marker";
    let root = tempdir().unwrap();
    let workspace = root.path().join(secret_path_marker);
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    let startup = json!({
        "type": "start",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "profile": "desktop",
        "workspaceRoot": workspace,
        "appDataRoot": app_data,
        "cacheRoot": cache,
        "workspaceState": workspace_state(&workspace),
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    process.read_stdout_line(PROCESS_TIMEOUT);

    process.write_raw(format!("{}\n", startup).as_bytes());
    let output = process.wait_for_output(PROCESS_TIMEOUT);
    let stdout = utf8(&output.stdout);
    let stderr = utf8(&output.stderr);

    assert!(!output.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert_eq!(stderr.trim(), "QingYu Kernel startup failed.");
    assert!(!stderr.contains(VALID_CREDENTIAL));
    assert!(!stderr.contains(secret_path_marker));
}

#[test]
fn malformed_or_oversized_control_frames_fail_generically() {
    let controls = [
        "{\n".to_owned(),
        format!(
            "{}\n",
            json!({
                "type": "shutdown",
                "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
                "unknownField": "private-control-marker",
            })
        ),
        format!("{}\n", "x".repeat(64 * 1024 + 1)),
    ];

    for control in controls {
        let (startup, _root) = desktop_startup_fixture();
        let mut process = KernelProcess::spawn(&startup.to_string());
        process.read_stdout_line(PROCESS_TIMEOUT);
        process.write_raw(control.as_bytes());

        let output = process.wait_for_output(PROCESS_TIMEOUT);
        let stdout = utf8(&output.stdout);
        let stderr = utf8(&output.stderr);
        assert!(!output.status.success());
        assert_eq!(stdout.lines().count(), 1);
        assert_eq!(stderr.trim(), "QingYu Kernel startup failed.");
        assert!(!stderr.contains("private-control-marker"));
    }
}

#[test]
fn invalid_startup_framing_fails_generically() {
    let (mut startup, _root) = desktop_startup_fixture();
    startup["unknownField"] = json!("private-unknown-field-marker");
    let (mut future_version, _future_root) = desktop_startup_fixture();
    future_version["protocolVersion"] = json!(NATIVE_HOST_PROTOCOL_VERSION + 1);
    let (mut missing_workspace_state, _missing_workspace_root) = desktop_startup_fixture();
    missing_workspace_state
        .as_object_mut()
        .unwrap()
        .remove("workspaceState");
    let (mut invalid_root_binding, _invalid_binding_root) = desktop_startup_fixture();
    invalid_root_binding["workspaceState"]["rootBinding"] = json!("private-invalid-binding");
    let cases = [
        FramedInput::line(startup.to_string()),
        FramedInput::line(future_version.to_string()),
        FramedInput::line(missing_workspace_state.to_string()),
        FramedInput::line(invalid_root_binding.to_string()),
        FramedInput::unterminated(
            json!({
                "type": "start",
                "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
                "profile": "desktop",
                "workspaceRoot": "/unterminated-private-path-marker",
                "appDataRoot": "/unterminated-private-app-data-marker",
                "cacheRoot": "/unterminated-private-cache-marker",
                "workspaceState": unbound_workspace_state(),
                "origin": "tauri://localhost",
                "credential": VALID_CREDENTIAL,
            })
            .to_string(),
        ),
        FramedInput::line("x".repeat(64 * 1024 + 1)),
    ];

    for input in cases {
        let output = KernelProcess::spawn_framed(input).wait_for_output(PROCESS_TIMEOUT);
        let stdout = utf8(&output.stdout);
        let stderr = utf8(&output.stderr);

        assert!(!output.status.success());
        assert!(
            stdout.is_empty(),
            "failed startup emitted stdout: {stdout:?}"
        );
        assert_eq!(stderr.trim(), "QingYu Kernel startup failed.");
        assert!(!stderr.contains(VALID_CREDENTIAL));
        assert!(!stderr.contains("private"));
    }
}

#[test]
fn invalid_or_missing_credentials_fail_generically_without_secret_disclosure() {
    let secret = "invalid-private-launch-secret";
    let cases = [
        json!({
            "type": "start",
            "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
            "profile": "desktop",
            "workspaceRoot": "/not/observed/before-credential-validation",
            "appDataRoot": "/not/observed/before-credential-validation-app-data",
            "cacheRoot": "/not/observed/before-credential-validation-cache",
            "workspaceState": unbound_workspace_state(),
            "origin": "tauri://localhost",
            "credential": secret,
        }),
        json!({
            "type": "start",
            "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
            "profile": "desktop",
            "workspaceRoot": "/missing-credential-secret-marker",
            "appDataRoot": "/missing-credential-secret-marker-app-data",
            "cacheRoot": "/missing-credential-secret-marker-cache",
            "workspaceState": unbound_workspace_state(),
            "origin": "tauri://localhost",
        }),
    ];

    for startup in cases {
        let output = KernelProcess::spawn(&startup.to_string()).wait_for_output(PROCESS_TIMEOUT);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();

        assert!(!output.status.success());
        assert!(
            stdout.is_empty(),
            "failed startup emitted stdout: {stdout:?}"
        );
        assert_eq!(stderr.trim(), "QingYu Kernel startup failed.");
        assert!(!stderr.contains(secret));
        assert!(!stderr.contains("missing-credential-secret-marker"));
    }
}

fn desktop_startup_fixture() -> (Value, tempfile::TempDir) {
    let root = tempdir().unwrap();
    let workspace = root.path().join("workspace");
    let app_data = root.path().join("app-data");
    let cache = root.path().join("cache");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&app_data).unwrap();
    std::fs::create_dir(&cache).unwrap();
    (
        json!({
            "type": "start",
            "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
            "profile": "desktop",
            "workspaceRoot": workspace,
            "appDataRoot": app_data,
            "cacheRoot": cache,
            "workspaceState": workspace_state(&workspace),
            "origin": "tauri://localhost",
            "credential": VALID_CREDENTIAL,
        }),
        root,
    )
}

fn workspace_state(workspace: &std::path::Path) -> Value {
    serde_json::to_value(NativeHostWorkspaceState::for_workspace(workspace, "Notes").unwrap())
        .unwrap()
}

fn unbound_workspace_state() -> Value {
    json!({
        "primaryWorkspace": {
            "schemaVersion": 1,
            "revisionSeed": "8b14d937-76b2-4776-9ae4-a9c6e0c403c4",
            "displayName": "Notes",
        },
        "rootBinding": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    })
}

fn live_probe(port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
    write!(
        stream,
        "GET /api/v1/health/live HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.flush().unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn authorized_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(IO_TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {VALID_CREDENTIAL}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    stream.flush().unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap()
}

struct KernelProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_lines: mpsc::Receiver<String>,
    stdout_output: mpsc::Receiver<Vec<u8>>,
    stderr_output: mpsc::Receiver<Vec<u8>>,
}

impl KernelProcess {
    fn spawn(startup_json: &str) -> Self {
        Self::spawn_framed(FramedInput::line(startup_json.to_owned()))
    }

    fn spawn_framed(input: FramedInput) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qingyu-kernel"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(input.bytes.as_slice()).unwrap();
        stdin.flush().unwrap();
        let stdin = input.keep_open.then_some(stdin);
        let (stdout_lines_sender, stdout_lines) = mpsc::channel();
        let (stdout_output_sender, stdout_output) = mpsc::sync_channel(1);
        let stdout = child.stdout.take().unwrap();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut output = Vec::new();
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        output.extend_from_slice(&line);
                        let _send_result =
                            stdout_lines_sender.send(String::from_utf8_lossy(&line).into_owned());
                    }
                }
            }
            let _send_result = stdout_output_sender.send(output);
        });
        let (stderr_output_sender, stderr_output) = mpsc::sync_channel(1);
        let mut stderr = child.stderr.take().unwrap();
        thread::spawn(move || {
            let mut output = Vec::new();
            let _read_result = stderr.read_to_end(&mut output);
            let _send_result = stderr_output_sender.send(output);
        });
        Self {
            child: Some(child),
            stdin,
            stdout_lines,
            stdout_output,
            stderr_output,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().unwrap()
    }

    fn read_stdout_line(&mut self, timeout: Duration) -> String {
        self.stdout_lines
            .recv_timeout(timeout)
            .expect("kernel did not report readiness before timeout")
    }

    fn write_shutdown(&mut self) {
        let stdin = self.stdin.as_mut().expect("kernel control lease is closed");
        NativeHostControl::write_shutdown_json_line(stdin).unwrap();
    }

    fn write_raw(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().expect("kernel control lease is closed");
        stdin.write_all(bytes).unwrap();
        stdin.flush().unwrap();
    }

    fn close_control(&mut self) {
        self.stdin.take();
    }

    fn wait_for_output(mut self, timeout: Duration) -> Output {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child_mut().try_wait().unwrap() {
                let _waited_status = self.child.take().unwrap().wait().unwrap();
                let stdout = self.stdout_output.recv_timeout(IO_TIMEOUT).unwrap();
                let stderr = self.stderr_output.recv_timeout(IO_TIMEOUT).unwrap();
                return output(status, stdout, stderr);
            }
            assert!(
                Instant::now() < deadline,
                "kernel process did not exit before timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

struct FramedInput {
    bytes: Vec<u8>,
    keep_open: bool,
}

impl FramedInput {
    fn line(input: String) -> Self {
        let mut bytes = input.into_bytes();
        bytes.push(b'\n');
        Self {
            bytes,
            keep_open: true,
        }
    }

    fn unterminated(input: String) -> Self {
        Self {
            bytes: input.into_bytes(),
            keep_open: false,
        }
    }
}

fn output(status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
    Output {
        status,
        stdout,
        stderr,
    }
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

impl Drop for KernelProcess {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_status)) => {}
                Ok(None) | Err(_) => {
                    let _kill_result = child.kill();
                    let _wait_result = child.wait();
                }
            }
        }
    }
}
