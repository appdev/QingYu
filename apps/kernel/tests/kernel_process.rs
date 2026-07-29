use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::TcpStream,
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tempfile::tempdir;

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
        "profile": "desktop",
        "workspaceRoot": workspace,
        "appDataRoot": app_data,
        "cacheRoot": cache,
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness_line = process.read_stdout_line(PROCESS_TIMEOUT);
    let readiness: Value = serde_json::from_str(readiness_line.trim()).unwrap();

    assert_eq!(
        readiness.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["instanceId", "port"]
    );
    assert!(readiness["instanceId"].as_str().is_some());
    assert!(!readiness_line.contains(VALID_CREDENTIAL));

    let port = readiness["port"].as_u64().unwrap();
    let response = live_probe(u16::try_from(port).unwrap());
    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected live response: {response}"
    );
    assert!(response.contains(r#""status":"live""#));
    assert_eq!(process.child_mut().try_wait().unwrap(), None);
}

#[test]
fn invalid_or_missing_credentials_fail_generically_without_secret_disclosure() {
    let secret = "invalid-private-launch-secret";
    let cases = [
        json!({
            "profile": "desktop",
            "workspaceRoot": "/not/observed/before-credential-validation",
            "appDataRoot": "/not/observed/before-credential-validation-app-data",
            "cacheRoot": "/not/observed/before-credential-validation-cache",
            "origin": "tauri://localhost",
            "credential": secret,
        }),
        json!({
            "profile": "desktop",
            "workspaceRoot": "/missing-credential-secret-marker",
            "appDataRoot": "/missing-credential-secret-marker-app-data",
            "cacheRoot": "/missing-credential-secret-marker-cache",
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

struct KernelProcess {
    child: Option<Child>,
}

impl KernelProcess {
    fn spawn(startup_json: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_qingyu-kernel"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(startup_json.as_bytes()).unwrap();
        drop(stdin);
        Self { child: Some(child) }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().unwrap()
    }

    fn read_stdout_line(&mut self, timeout: Duration) -> String {
        let stdout = self.child_mut().stdout.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _result = sender.send(result);
        });
        receiver
            .recv_timeout(timeout)
            .expect("kernel did not report readiness before timeout")
            .expect("failed to read kernel readiness")
    }

    fn wait_for_output(mut self, timeout: Duration) -> Output {
        let deadline = Instant::now() + timeout;
        loop {
            if self.child_mut().try_wait().unwrap().is_some() {
                return self.child.take().unwrap().wait_with_output().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "kernel process did not exit before timeout"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
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
