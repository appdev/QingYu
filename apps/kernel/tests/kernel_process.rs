use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::TcpStream,
    path::Path,
    process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tempfile::tempdir;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const VALID_CREDENTIAL: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const NATIVE_HOST_PROTOCOL_VERSION: u64 = 1;

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
        "origin": "tauri://localhost",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness_line = process.read_stdout_line(PROCESS_TIMEOUT);
    let readiness: Value = serde_json::from_str(readiness_line.trim()).unwrap();

    assert_eq!(
        readiness.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["instanceId", "port", "protocolVersion", "type"]
    );
    assert_eq!(readiness["type"], "ready");
    assert_eq!(readiness["protocolVersion"], NATIVE_HOST_PROTOCOL_VERSION);
    assert!(readiness["instanceId"].as_str().is_some());
    assert!(!readiness_line.contains(VALID_CREDENTIAL));

    let port = readiness["port"].as_u64().unwrap();
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
#[ignore = "run inside an isolated Server container with an empty writable /data mount"]
fn server_startup_uses_fixed_data_state_and_reports_process_readiness() {
    let data_root = Path::new("/data");
    assert!(data_root.is_dir(), "the container must mount /data");
    assert!(
        std::fs::read_dir(data_root).unwrap().next().is_none(),
        "the container integration test requires an empty /data mount"
    );
    let startup = json!({
        "type": "start",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
        "profile": "server",
        "origin": "http://127.0.0.1:3000",
        "credential": VALID_CREDENTIAL,
    });
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness: Value =
        serde_json::from_str(process.read_stdout_line(PROCESS_TIMEOUT).trim()).unwrap();
    let port = u16::try_from(readiness["port"].as_u64().unwrap()).unwrap();

    let runtime = authorized_get(port, "/api/v1/runtime");
    assert!(runtime.starts_with("HTTP/1.1 200 OK\r\n"), "{runtime}");
    let runtime_body: Value = serde_json::from_str(response_body(&runtime)).unwrap();
    assert_eq!(runtime_body["profile"], "server");
    assert_eq!(runtime_body["capabilities"]["settings"], true);
    assert!(Path::new("/data/workspace").is_dir());
    assert!(Path::new("/data/config").is_dir());
    assert!(Path::new("/data/state/settings.json").is_file());
    assert!(Path::new("/data/logs").is_dir());
    assert_eq!(process.child_mut().try_wait().unwrap(), None);
}

#[test]
fn explicit_shutdown_frame_stops_the_process_cleanly_without_extra_stdout() {
    let (startup, _root) = desktop_startup_fixture();
    let mut process = KernelProcess::spawn(&startup.to_string());
    let readiness = process.read_stdout_line(PROCESS_TIMEOUT);
    assert!(readiness.contains(r#""type":"ready""#));

    process.write_control(&json!({
        "type": "shutdown",
        "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
    }));
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
fn duplicate_or_malformed_control_frames_fail_without_disclosing_startup_data() {
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
    let cases = [
        FramedInput::line(startup.to_string()),
        FramedInput::line(future_version.to_string()),
        FramedInput::unterminated(
            json!({
                "type": "start",
                "protocolVersion": NATIVE_HOST_PROTOCOL_VERSION,
                "profile": "desktop",
                "workspaceRoot": "/unterminated-private-path-marker",
                "appDataRoot": "/unterminated-private-app-data-marker",
                "cacheRoot": "/unterminated-private-cache-marker",
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
            "origin": "tauri://localhost",
            "credential": VALID_CREDENTIAL,
        }),
        root,
    )
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

    fn write_control(&mut self, control: &Value) {
        self.write_raw(format!("{control}\n").as_bytes());
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
