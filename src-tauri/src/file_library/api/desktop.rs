use super::*;
use std::net::Ipv4Addr;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

pub(in crate::file_library) const MCP_PORT: u16 = 48787;

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

fn library_config_path(config: &Path, root: &Path, extension: &str) -> PathBuf {
    let key = Sha256::digest(root.as_os_str().as_encoded_bytes());
    config.join(format!("mcp-{key:x}.{extension}"))
}

fn token_for_library(config: &Path, root: &Path) -> Result<String> {
    let path = library_config_path(config, root, "token");
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

fn set_auto_start(config: &Path, root: &Path, enabled: bool) -> Result<()> {
    let path = library_config_path(config, root, "enabled");
    if enabled {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(error)?;
    } else if let Err(reason) = fs::remove_file(path) {
        if reason.kind() != io::ErrorKind::NotFound {
            return Err(error(reason));
        }
    }
    Ok(())
}

pub(in crate::file_library) async fn restore(
    running: &mut Option<Listener>,
    store: &DiskLibrary,
    config: PathBuf,
    port: u16,
) -> Result<()> {
    let enabled = library_config_path(&config, &store.root, "enabled");
    if running.is_none() && enabled.try_exists().map_err(error)? {
        *running = Some(start(store.clone(), config, port).await?);
    }
    Ok(())
}

async fn start(store: DiskLibrary, config: PathBuf, port: u16) -> Result<Listener> {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Err(reason) if reason.kind() == io::ErrorKind::AddrInUse => {
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await
        }
        result => result,
    }
    .map_err(|reason| format!("无法启动 MCP 服务：{reason}"))?;
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
        sync: None,
        config: None,
        account: Default::default(),
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
    if let Some(enabled) = enabled {
        let config = window.app_handle().path().app_config_dir().map_err(error)?;
        if enabled {
            if running.is_none() {
                let store = library
                    .0
                    .lock()
                    .map_err(error)?
                    .clone()
                    .ok_or("请先打开笔记库")?;
                let listener = start(store, config.clone(), MCP_PORT).await?;
                set_auto_start(&config, &listener.root, true)?;
                *running = Some(listener);
            }
        } else {
            if let Some(store) = library.0.lock().map_err(error)?.as_ref() {
                set_auto_start(&config, &store.root, false)?;
            }
            stop(&mut running).await;
        }
    }
    Ok(running.as_ref().map(|listener| listener.connection.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_listener_restores_only_for_a_library_with_auto_start_enabled() {
        let config = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        let other_library = tempfile::tempdir().unwrap();
        let store = DiskLibrary::open(library.path().into(), None).unwrap();
        let other = DiskLibrary::open(other_library.path().into(), None).unwrap();
        let mut running = None;
        let token = token_for_library(config.path(), &store.root).unwrap();
        restore(&mut running, &store, config.path().into(), 0)
            .await
            .unwrap();
        assert!(running.is_none());

        set_auto_start(config.path(), &store.root, true).unwrap();
        restore(&mut running, &other, config.path().into(), 0)
            .await
            .unwrap();
        assert!(running.is_none());
        restore(&mut running, &store, config.path().into(), 0)
            .await
            .unwrap();
        assert_eq!(running.as_ref().unwrap().connection.token, token);
        stop(&mut running).await;

        set_auto_start(config.path(), &store.root, false).unwrap();
        restore(&mut running, &store, config.path().into(), 0)
            .await
            .unwrap();
        assert!(running.is_none());
    }

    #[tokio::test]
    async fn desktop_listener_authenticates_requests_and_stops_with_a_reusable_library_token() {
        let library = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let store = DiskLibrary::open(library.path().into(), None).unwrap();
        let mut running = Some(start(store.clone(), config.path().into(), 0).await.unwrap());
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
        let old_address: SocketAddr = connection
            .url
            .trim_start_matches("http://")
            .trim_end_matches("/mcp")
            .parse()
            .unwrap();
        let occupied = TcpListener::bind(old_address).await.unwrap();
        let listener = start(store.clone(), config.path().into(), old_address.port())
            .await
            .unwrap();
        assert_ne!(listener.connection.url, connection.url);
        assert_eq!(listener.connection.token, connection.token);
        stop(&mut Some(listener)).await;

        drop(occupied);
        let listener = start(store, config.path().into(), old_address.port())
            .await
            .unwrap();
        assert_eq!(listener.connection.url, connection.url);
        stop(&mut Some(listener)).await;
    }
}
