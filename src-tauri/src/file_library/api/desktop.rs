use super::*;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

const ADDRESS: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8787);

#[derive(Default)]
pub(crate) struct LocalMcp(pub(in crate::file_library) tokio::sync::Mutex<Option<Listener>>);

pub(in crate::file_library) struct Listener {
    pub(in crate::file_library) root: PathBuf,
    connection: Connection,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Clone, Serialize)]
pub(crate) struct Connection {
    url: String,
    token: String,
}

fn token_for_library(config: &Path, root: &Path) -> Result<String> {
    let key = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let path = config.join(format!("mcp-{key:x}.token"));
    match fs::read_to_string(&path) {
        Ok(token) => {
            token_hash(&token).map_err(|_| "本地 MCP 令牌无效")?;
            Ok(token)
        }
        Err(reason) if reason.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(config).map_err(error)?;
            let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            atomic(&path, token.as_bytes(), false)?;
            Ok(token)
        }
        Err(reason) => Err(error(reason)),
    }
}

async fn start(store: DiskLibrary, config: PathBuf, address: SocketAddr) -> Result<Listener> {
    let listener = TcpListener::bind(address)
        .await
        .map_err(|reason| format!("无法启动 API 服务（{address}）：{reason}"))?;
    let url = format!("http://{}/mcp", listener.local_addr().map_err(error)?);
    let root = store.root.clone();
    let library = root.clone();
    let token = tokio::task::spawn_blocking(move || token_for_library(&config, &library))
        .await
        .map_err(error)??;
    let state = Arc::new(Service {
        store: Mutex::new(store),
        token: Some(token_hash(&token)?),
        read_token: None,
        web: None,
    });
    let (shutdown, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, router(state))
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await;
    });
    Ok(Listener {
        root,
        connection: Connection { url, token },
        shutdown,
        task,
    })
}

pub(in crate::file_library) async fn stop(running: &mut Option<Listener>) {
    if let Some(listener) = running.take() {
        let _ = listener.shutdown.send(());
        // Closing the listener need not wait for a client to finish uploading a request.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), listener.task).await;
    }
}

#[tauri::command]
pub(crate) async fn local_mcp(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, LocalMcp>,
    library: tauri::State<'_, FileLibrary>,
    enabled: Option<bool>,
) -> Result<Option<Connection>> {
    if window.label() != "settings" {
        return Err("请在轻羽设置中管理 MCP".into());
    }
    let mut running = state.0.lock().await;
    if running
        .as_ref()
        .is_some_and(|listener| listener.task.is_finished())
    {
        *running = None;
    }
    match enabled {
        Some(false) => stop(&mut running).await,
        Some(true) if running.is_none() => {
            let store = library
                .0
                .lock()
                .map_err(error)?
                .clone()
                .ok_or("请先打开笔记库")?;
            let config = window.app_handle().path().app_config_dir().map_err(error)?;
            *running = Some(start(store, config, ADDRESS).await?);
        }
        _ => {}
    }
    Ok(running.as_ref().map(|listener| listener.connection.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_listener_authenticates_requests_and_stops_with_a_reusable_library_token() {
        let library = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let store = DiskLibrary::open(library.path().into(), None).unwrap();
        let mut address = ADDRESS;
        address.set_port(0);
        let mut running = Some(
            start(store.clone(), config.path().into(), address)
                .await
                .unwrap(),
        );
        let connection = running.as_ref().unwrap().connection.clone();
        assert_ne!(
            token_for_library(config.path(), config.path()).unwrap(),
            connection.token
        );
        let client = reqwest::Client::new();
        let request = || {
            client
                .post(&connection.url)
                .header("Content-Type", "application/json")
                .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
        };
        assert_eq!(request().send().await.unwrap().status().as_u16(), 401);
        let response = request()
            .bearer_auth(&connection.token)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert!(response.text().await.unwrap().contains("update_note"));
        let response = client.post(&connection.url).header("Content-Type", "application/json")
            .bearer_auth(&connection.token)
            .body(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"create_note","arguments":{"folderId":"default","title":"Test note","source":"# Hello"}
            }}).to_string())
            .send().await.unwrap();
        assert!(response.text().await.unwrap().contains("structuredContent"));
        assert_eq!(
            fs::read_to_string(library.path().join("Test note.md")).unwrap(),
            "# Hello"
        );

        stop(&mut running).await;
        assert!(running.is_none());
        assert!(request()
            .bearer_auth(&connection.token)
            .send()
            .await
            .is_err());
        let old_address = connection
            .url
            .trim_start_matches("http://")
            .trim_end_matches("/mcp")
            .parse()
            .unwrap();
        let listener = start(store, config.path().into(), old_address)
            .await
            .unwrap();
        assert_eq!(listener.connection.token, connection.token);
        stop(&mut Some(listener)).await;
    }
}
