//! HTTP transport over the same file transactions used by the desktop editor.
use super::*;
use axum::{
    extract::{
        rejection::JsonRejection, DefaultBodyLimit, Path as RoutePath, Query, Request, State,
    },
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};
use subtle::ConstantTimeEq;

const MAX_BODY: usize = 4 * 1024 * 1024;
mod web;
mod browser_library;
mod mcp;
#[cfg(all(feature = "app", desktop))]
pub(crate) mod desktop;
type ApiResult<T> = std::result::Result<T, ApiError>;

struct ApiError(StatusCode, &'static str, &'static str);

impl ApiError {
    fn bad_request(message: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message)
    }
    fn not_found() -> Self {
        Self(
            StatusCode::NOT_FOUND,
            "not_found",
            "Document or folder not found",
        )
    }
    fn internal() -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The operation could not be completed",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.0,
            Json(json!({"error": {"code": self.1, "message": self.2}})),
        )
            .into_response();
        if self.0 == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
        }
        response
    }
}

struct Service {
    store: Mutex<DiskLibrary>,
    token: Option<[u8; 32]>,
    read_token: Option<[u8; 32]>,
    web: Option<web::Web>,
}

fn token_hash(token: &str) -> std::result::Result<[u8; 32], &'static str> {
    if !(32..=4096).contains(&token.len()) || !token.bytes().all(|b| (33..=126).contains(&b)) {
        return Err("API tokens must contain 32–4096 visible ASCII characters without spaces");
    }
    Ok(Sha256::digest(token.as_bytes()).into())
}

impl Service {
    fn open(
        root: PathBuf,
        token: Option<&str>,
        read_token: Option<&str>,
        web: Option<web::Web>,
    ) -> Result<Arc<Self>> {
        if token.is_none() && web.is_none() {
            return Err("Set QINGYU_API_TOKEN or enable Web with QINGYU_WEB_PASSWORD".into());
        }
        let token = token.map(token_hash).transpose()?;
        let read_token = read_token.map(token_hash).transpose()?;
        if read_token
            .zip(token)
            .is_some_and(|(read, write)| bool::from(read.ct_eq(&write)))
        {
            return Err("Read-only and read/write tokens must be different".into());
        }
        Ok(Arc::new(Self {
            store: Mutex::new(DiskLibrary::open(root, None)?),
            token,
            read_token,
            web,
        }))
    }
}

fn bearer_access(state: &Service, headers: &HeaderMap) -> (bool, bool) {
    let token = if headers
        .get_all(header::AUTHORIZATION)
        .iter()
        .count()
        == 1
    {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
            .map(|(_, token)| <[u8; 32]>::from(Sha256::digest(token.as_bytes())))
    } else {
        None
    };
    let write = token
        .zip(state.token)
        .is_some_and(|(hash, expected)| bool::from(hash.ct_eq(&expected)));
    let read = token
        .zip(state.read_token)
        .is_some_and(|(hash, expected)| bool::from(hash.ct_eq(&expected)));
    (write, read)
}

async fn authorize(State(state): State<Arc<Service>>, request: Request, next: Next) -> Response {
    let (write, read) = bearer_access(&state, request.headers());
    // Explicit Bearer credentials never inherit the browser's higher privileges.
    let session = !request.headers().contains_key(header::AUTHORIZATION)
        && state
            .web
            .as_ref()
            .is_some_and(|web| web.authenticated(request.headers()));
    let mut response = if !write && !read && !session {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A valid Bearer token is required",
        )
        .into_response()
    } else if session
        && !matches!(*request.method(), Method::GET | Method::HEAD)
        && !state.web.as_ref().unwrap().same_origin(request.headers())
    {
        web::forbidden().into_response()
    } else if !write && !session && !matches!(*request.method(), Method::GET | Method::HEAD) {
        ApiError(
            StatusCode::FORBIDDEN,
            "read_only_token",
            "This token only permits reading",
        )
        .into_response()
    } else {
        next.run(request).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

fn router(state: Arc<Service>) -> Router {
    let api = Router::new()
        .route("/v1/folders", get(folders))
        .route("/v1/notes", get(notes).post(create))
        .route("/v1/notes/{id}", get(read).put(update))
        .fallback(|| async { ApiError::not_found() })
        .layer(DefaultBodyLimit::max(MAX_BODY));
    let api = Router::new().nest("/api", api.merge(browser_library::router(state.clone())));
    let auth = middleware::from_fn_with_state(state.clone(), authorize);
    if state.web.is_some() {
        web::router(&state)
            .merge(api.layer(auth))
            .merge(mcp::router(state.clone()))
            .layer(middleware::from_fn_with_state(state.clone(), web::gate))
            .with_state(state)
    } else {
        api.fallback(|| async { ApiError::not_found() })
            .layer(auth)
            .merge(mcp::router(state.clone()))
            .with_state(state)
    }
}

async fn with_store<T: Send + 'static>(
    state: Arc<Service>,
    action: impl FnOnce(&mut DiskLibrary) -> ApiResult<T> + Send + 'static,
) -> ApiResult<T> {
    tokio::task::spawn_blocking(move || {
        let mut store = state.store.lock().map_err(|_| ApiError::internal())?;
        // Refresh through the existing reconciler before every request, including writes.
        store.reconcile(None).map_err(|reason| {
            eprintln!("API library refresh failed: {reason}");
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "library_unavailable",
                "The library is unavailable; retry later",
            )
        })?;
        action(&mut store)
    })
    .await
    .map_err(|_| ApiError::internal())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderInfo {
    id: String,
    name: String,
    path: String,
    parent_id: Option<String>,
}

async fn folders(State(state): State<Arc<Service>>) -> ApiResult<Json<serde_json::Value>> {
    with_store(state, |store| {
        let mut folders = store
            .data
            .folders
            .iter()
            .map(|folder| {
                let parent = folder
                    .path
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent);
                FolderInfo {
                    id: folder.id.clone(),
                    name: folder.name.clone(),
                    path: folder.path.clone(),
                    parent_id: if folder.path.is_empty() {
                        None
                    } else {
                        store
                            .data
                            .folders
                            .iter()
                            .find(|f| f.path == parent)
                            .map(|f| f.id.clone())
                    },
                }
            })
            .collect::<Vec<_>>();
        folders.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Json(json!({"items": folders})))
    })
    .await
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuery {
    folder_id: Option<String>,
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteInfo {
    id: String,
    folder_id: String,
    title: String,
    path: String,
    created_at: f64,
    updated_at: f64,
    read_only: bool,
    version: String,
    excerpt: String,
}

fn note_info(note: &Note) -> ApiResult<NoteInfo> {
    let bytes = serde_json::to_vec(note).map_err(|_| ApiError::internal())?;
    Ok(NoteInfo {
        id: note.id.clone(),
        folder_id: note.folder_id.clone(),
        title: note.title.clone().unwrap_or_default(),
        path: note.path.clone(),
        created_at: note.created_at,
        updated_at: note.updated_at,
        read_only: note.read_only,
        version: format!("{:x}", Sha256::digest(bytes)),
        excerpt: note.source.chars().take(160).collect(),
    })
}

async fn notes(
    State(state): State<Arc<Service>>,
    query: std::result::Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Query(query) = query.map_err(|_| ApiError::bad_request("Invalid query parameters"))?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=200).contains(&limit)
        || query.q.as_ref().is_some_and(|s| s.len() > 2000)
        || [&query.folder_id, &query.cursor]
            .iter()
            .any(|v| v.as_ref().is_some_and(|s| s.is_empty() || s.len() > 500))
    {
        return Err(ApiError::bad_request(
            "limit must be 1–200; q at most 2000 bytes; IDs at most 500 bytes",
        ));
    }
    with_store(state, move |store| {
        if query
            .folder_id
            .as_ref()
            .is_some_and(|id| !store.data.folders.iter().any(|f| &f.id == id))
        {
            return Err(ApiError::not_found());
        }
        let search = query.q.as_deref().unwrap_or("").trim().to_lowercase();
        let mut matches = store
            .data
            .notes
            .iter()
            .filter(|note| {
                !note.trashed
                    && query
                        .folder_id
                        .as_ref()
                        .is_none_or(|id| &note.folder_id == id)
                    && query.cursor.as_ref().is_none_or(|cursor| &note.id > cursor)
                    && (search.is_empty()
                        || note.source.to_lowercase().contains(&search)
                        || note
                            .title
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&search))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|a, b| a.id.cmp(&b.id));
        let items = matches
            .iter()
            .take(limit)
            .map(|note| note_info(note))
            .collect::<ApiResult<Vec<_>>>()?;
        let next_cursor = if matches.len() > limit {
            items.last().map(|note| &note.id)
        } else {
            None
        };
        Ok(Json(json!({"items": items, "nextCursor": next_cursor})))
    })
    .await
}

fn find_note<'a>(store: &'a DiskLibrary, id: &str) -> ApiResult<&'a Note> {
    store
        .data
        .notes
        .iter()
        .find(|note| note.id == id && !note.trashed)
        .ok_or_else(ApiError::not_found)
}

fn document(note: &Note, status: StatusCode) -> ApiResult<Response> {
    let info = note_info(note)?;
    let etag = format!("\"{}\"", info.version);
    let mut body = serde_json::to_value(info).map_err(|_| ApiError::internal())?;
    body["source"] = json!(note.source);
    Ok((status, [(header::ETAG, etag)], Json(body)).into_response())
}

async fn read(
    State(state): State<Arc<Service>>,
    RoutePath(id): RoutePath<String>,
) -> ApiResult<Response> {
    with_store(state, move |store| {
        document(find_note(store, &id)?, StatusCode::OK)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateNote {
    folder_id: String,
    title: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateNote {
    source: String,
    title: Option<String>,
}

fn validate_input(title: Option<&str>, source: &str) -> ApiResult<()> {
    if source.len() > MAX_BODY
        || title.is_some_and(|title| {
            title.trim().is_empty()
                || title.chars().count() > 500
                || title.contains('\\')
                || name(title).is_err()
        })
    {
        return Err(ApiError::bad_request(
            "Invalid document title or content exceeds 4 MiB",
        ));
    }
    Ok(())
}

fn commit(store: &mut DiskLibrary, next: Library, intent: &str) -> ApiResult<()> {
    store.commit(next, intent).map_err(|reason| {
        eprintln!("API document commit failed: {reason}");
        ApiError(
            StatusCode::CONFLICT,
            "storage_conflict",
            "The library changed or cannot be written; read it again before retrying",
        )
    })
}

fn payload<T>(value: std::result::Result<Json<T>, JsonRejection>) -> ApiResult<T> {
    value.map(|Json(value)| value).map_err(|error| {
        ApiError(
            error.status(),
            "invalid_json",
            "Expected a valid JSON document within the 4 MiB request limit",
        )
    })
}

async fn create(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<CreateNote>, JsonRejection>,
) -> ApiResult<Response> {
    let body = payload(body)?;
    validate_input(Some(&body.title), &body.source)?;
    with_store(state, move |store| {
        if !store
            .data
            .folders
            .iter()
            .any(|folder| folder.id == body.folder_id)
        {
            return Err(ApiError::not_found());
        }
        let mut next = store.data.clone();
        let id = id();
        let time = now();
        next.notes.push(Note {
            id: id.clone(),
            folder_id: body.folder_id,
            source: body.source,
            title: Some(body.title.trim().into()),
            created_at: time,
            updated_at: time,
            pinned: false,
            trashed: false,
            conflict_of: None,
            path: String::new(),
            read_only: false,
            filename: None,
            share: None,
        });
        commit(store, next, "create")?;
        let mut response = document(find_note(store, &id)?, StatusCode::CREATED)?;
        response.headers_mut().insert(
            header::LOCATION,
            format!("/api/v1/notes/{id}")
                .parse()
                .map_err(|_| ApiError::internal())?,
        );
        Ok(response)
    })
    .await
}

async fn update(
    State(state): State<Arc<Service>>,
    RoutePath(id): RoutePath<String>,
    headers: HeaderMap,
    body: std::result::Result<Json<UpdateNote>, JsonRejection>,
) -> ApiResult<Response> {
    let expected = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError(
            StatusCode::PRECONDITION_REQUIRED,
            "version_required",
            "Send the document ETag in If-Match",
        ))?
        .to_str()
        .map_err(|_| ApiError::bad_request("Invalid If-Match"))?
        .to_string();
    if headers.get_all(header::IF_MATCH).iter().count() != 1
        || expected.len() != 66
        || !expected.starts_with('"')
        || !expected.ends_with('"')
        || !expected.as_bytes()[1..65]
            .iter()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(ApiError::bad_request(
            "If-Match must contain one quoted document version",
        ));
    }
    let body = payload(body)?;
    validate_input(body.title.as_deref(), &body.source)?;
    with_store(state, move |store| {
        let old = find_note(store, &id)?;
        if format!("\"{}\"", note_info(old)?.version) != expected {
            return Err(ApiError(
                StatusCode::PRECONDITION_FAILED,
                "version_conflict",
                "The document changed; read it again before updating",
            ));
        }
        if old.read_only {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "read_only_document",
                "This document cannot be modified",
            ));
        }
        let mut next = store.data.clone();
        let note = next
            .notes
            .iter_mut()
            .find(|note| note.id == id)
            .ok_or_else(ApiError::not_found)?;
        if note.source == body.source
            && body
                .title
                .as_ref()
                .is_none_or(|title| note.title.as_deref() == Some(title.trim()))
        {
            return document(old, StatusCode::OK);
        }
        note.source = body.source;
        note.updated_at = now();
        if let Some(title) = body.title {
            note.title = Some(title.trim().into());
        }
        commit(store, next, "edit")?;
        document(find_note(store, &id)?, StatusCode::OK)
    })
    .await
}

// Existing deployments can keep their pre-rename environment configuration.
fn environment(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("QINGYU_{name}"))
        .or_else(|| std::env::var_os(format!("MIAOYAN_{name}")))
}

/// Run without the `app` feature to avoid all Tauri and WebView dependencies.
pub async fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let root = environment("LIBRARY_DIR")
        .ok_or("Set QINGYU_LIBRARY_DIR to the persistent library directory")?;
    let token = environment("API_TOKEN")
        .map(|value| {
            value
                .into_string()
                .map_err(|_| "QINGYU_API_TOKEN must contain valid UTF-8")
        })
        .transpose()?;
    let read_token = environment("API_READ_TOKEN")
        .map(|value| {
            value
                .into_string()
                .map_err(|_| "QINGYU_API_READ_TOKEN must contain valid UTF-8")
        })
        .transpose()?;
    let addr: SocketAddr = environment("API_ADDR")
        .map(|value| {
            value
                .into_string()
                .map_err(|_| "QINGYU_API_ADDR must contain valid UTF-8")
        })
        .transpose()?
        .unwrap_or_else(|| "127.0.0.1:8787".into())
        .parse()?;
    let web = environment("WEB_PASSWORD")
        .map(|password| {
            let password = password
                .into_string()
                .map_err(|_| "QINGYU_WEB_PASSWORD must contain valid UTF-8")?;
            let secure = environment("WEB_SECURE_COOKIE")
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "QINGYU_WEB_SECURE_COOKIE must be true or false")
                })
                .transpose()?;
            let secure = match secure.as_deref() {
                None | Some("true") => true,
                Some("false") => false,
                _ => return Err("QINGYU_WEB_SECURE_COOKIE must be true or false".into()),
            };
            web::Web::open(
                PathBuf::from(environment("WEB_DIR").unwrap_or_else(|| "dist".into())),
                &password,
                secure,
            )
        })
        .transpose()?;
    let state = Service::open(
        PathBuf::from(root),
        token.as_deref(),
        read_token.as_deref(),
        web,
    )?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("QingYu API listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => eprintln!("Could not install termination handler: {error}"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

#[cfg(test)]
mod tests;
