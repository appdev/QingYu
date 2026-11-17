//! Browser operations share the desktop transactions, never accept an entire client library.
use super::*;
use crate::file_library::storage_usage::{StorageUsage, UsageTarget};

pub(super) fn router(state: Arc<Service>) -> Router<Arc<Service>> {
    Router::new()
        .route("/web/library", get(snapshot))
        .route("/web/storage-usage", get(storage_usage))
        .route("/web/commands", axum::routing::post(change))
        .route("/web/link-preview", axum::routing::post(link_preview).layer(DefaultBodyLimit::max(12 * 1024)))
        .route("/web/assets/{id}", get(asset).post(upload).layer(DefaultBodyLimit::max(assets::MAX_FILE * 4 + 65536)))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(state, browser_only))
}

async fn link_preview(
    body: std::result::Result<Json<crate::link_preview::PreviewRequest>, JsonRejection>,
) -> ApiResult<Json<Option<crate::link_preview::PreviewResource>>> {
    Ok(Json(crate::link_preview::fetch(payload(body)?).await))
}

async fn browser_only(State(state): State<Arc<Service>>, request: Request, next: Next) -> Response {
    // These richer editor routes do not expand the public AI token's scope to trash or metadata.
    if request.headers().contains_key(header::AUTHORIZATION)
        || !state
            .web
            .as_ref()
            .is_some_and(|web| web.authenticated(request.headers()))
    {
        return ApiError(
            StatusCode::UNAUTHORIZED,
            "session_required",
            "Sign in to use the editor",
        )
        .into_response();
    }
    next.run(request).await
}

fn library_version(store: &DiskLibrary) -> ApiResult<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&store.data).map_err(|_| ApiError::internal())?)
    ))
}

fn view(store: &DiskLibrary) -> ApiResult<Json<serde_json::Value>> {
    let versions: std::collections::BTreeMap<_, _> = store
        .data
        .notes
        .iter()
        .map(|note| Ok((note.id.clone(), note_info(note)?.version)))
        .collect::<ApiResult<_>>()?;
    Ok(Json(
        json!({ "folders": store.data.folders, "notes": store.data.notes,
        "versions": versions, "version": library_version(store)? }),
    ))
}

async fn snapshot(State(state): State<Arc<Service>>) -> ApiResult<Json<serde_json::Value>> {
    with_store(state, |store| view(store)).await
}

async fn storage_usage(State(state): State<Arc<Service>>) -> ApiResult<Json<StorageUsage>> {
    let target = {
        let store = state.store.lock().map_err(|_| ApiError::internal())?;
        UsageTarget::new(&store)
    };
    tokio::task::spawn_blocking(move || target.measure().map(Json).map_err(|_| ApiError::internal()))
        .await
        .map_err(|_| ApiError::internal())?
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Command {
    CreateNote {
        id: String,
        folder_id: String,
        title: String,
        source: String,
    },
    UpdateNote {
        id: String,
        version: String,
        source: String,
        title: Option<String>,
        pinned: bool,
        trashed: bool,
        folder_id: String,
    },
    DuplicateNote {
        id: String,
        version: String,
        new_id: String,
    },
    CreateFolder {
        parent_id: String,
        name: String,
    },
    RenameFolder {
        id: String,
        version: String,
        name: String,
    },
    MoveFolder {
        id: String,
        version: String,
        destination_id: String,
    },
}

fn versioned_note<'a>(store: &'a DiskLibrary, id: &str, version: &str) -> ApiResult<&'a Note> {
    let note = store
        .data
        .notes
        .iter()
        .find(|n| n.id == id)
        .ok_or_else(ApiError::not_found)?;
    if note_info(note)?.version != version {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "version_conflict",
            "The document changed; keep your draft and reload",
        ));
    }
    if note.read_only {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "read_only_document",
            "This document cannot be modified",
        ));
    }
    Ok(note)
}

fn check_id(store: &DiskLibrary, value: &str) -> ApiResult<()> {
    if Uuid::parse_str(value).is_err() || store.data.notes.iter().any(|n| n.id == value) {
        return Err(ApiError::bad_request("Invalid or duplicate note ID"));
    }
    Ok(())
}

async fn change(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<Command>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let command = payload(body)?;
    with_store(state, move |store| {
        match command {
            Command::CreateNote {
                id,
                folder_id,
                title,
                source,
            } => {
                check_id(store, &id)?;
                validate_input(if title.is_empty() { None } else { Some(&title) }, &source)?;
                if !store.data.folders.iter().any(|f| f.id == folder_id) {
                    return Err(ApiError::not_found());
                }
                let mut next = store.data.clone();
                let time = now();
                next.notes.push(Note {
                    id,
                    folder_id,
                    source,
                    title: (!title.is_empty()).then_some(title),
                    pinned: false,
                    trashed: false,
                    created_at: time,
                    updated_at: time,
                    conflict_of: None,
                    path: String::new(),
                    read_only: false,
                    filename: None,
                });
                commit(store, next, "create")?;
            }
            Command::UpdateNote {
                id,
                version,
                source,
                title,
                pinned,
                trashed,
                folder_id,
            } => {
                versioned_note(store, &id, &version)?;
                validate_input(title.as_deref(), &source)?;
                if !store.data.folders.iter().any(|f| f.id == folder_id) {
                    return Err(ApiError::not_found());
                }
                let mut next = store.data.clone();
                let note = next.notes.iter_mut().find(|n| n.id == id).unwrap();
                if note.source != source {
                    note.updated_at = now();
                }
                note.source = source;
                note.title = title;
                note.pinned = pinned;
                note.trashed = trashed;
                note.folder_id = folder_id;
                commit(store, next, "edit")?;
            }
            Command::DuplicateNote {
                id,
                version,
                new_id,
            } => {
                check_id(store, &new_id)?;
                let original = versioned_note(store, &id, &version)?.clone();
                let mut next = store.data.clone();
                let time = now();
                next.notes.push(Note {
                    id: new_id,
                    source: original.source,
                    folder_id: original.folder_id,
                    created_at: time,
                    updated_at: time,
                    title: original.title,
                    pinned: false,
                    trashed: false,
                    conflict_of: None,
                    path: String::new(),
                    filename: None,
                    read_only: false,
                });
                commit(store, next, &format!("duplicate:{id}"))?;
            }
            Command::CreateFolder { parent_id, name } => {
                store
                    .folder_change("create", &parent_id, &name, None)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "folder_conflict",
                            "The folder cannot be created",
                        )
                    })?;
            }
            Command::RenameFolder { id, version, name } => {
                if version != library_version(store)? {
                    return Err(ApiError(
                        StatusCode::PRECONDITION_FAILED,
                        "version_conflict",
                        "The library changed; reload before moving folders",
                    ));
                }
                store
                    .folder_change("rename", &id, &name, None)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "folder_conflict",
                            "The folder cannot be renamed",
                        )
                    })?;
            }
            Command::MoveFolder {
                id,
                version,
                destination_id,
            } => {
                if version != library_version(store)? {
                    return Err(ApiError(
                        StatusCode::PRECONDITION_FAILED,
                        "version_conflict",
                        "The library changed; reload before moving folders",
                    ));
                }
                let folder = store
                    .data
                    .folders
                    .iter()
                    .find(|f| f.id == destination_id)
                    .ok_or_else(ApiError::not_found)?;
                let destination = store.root.join(&folder.path);
                store
                    .folder_change(
                        "move",
                        &id,
                        destination.to_str().ok_or_else(ApiError::internal)?,
                        None,
                    )
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "folder_conflict",
                            "The folder cannot be moved",
                        )
                    })?;
            }
        }
        view(store)
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetQuery {
    path: String,
}

async fn asset(
    State(state): State<Arc<Service>>,
    RoutePath(id): RoutePath<String>,
    Query(query): Query<AssetQuery>,
) -> ApiResult<Response> {
    with_store(state, move |store| {
        let path = attachments::resolve(store, &id, &query.path).map_err(|_| ApiError::not_found())?;
        let content = attachments::read(&path).map_err(|_| ApiError::not_found())?;
        let mime = attachments::mime(&path);
        let filename: String = path.file_name().unwrap_or_default().to_string_lossy().as_bytes().iter().map(|byte| if byte.is_ascii_alphanumeric() || b"-_.".contains(byte) { (*byte as char).to_string() } else { format!("%{byte:02X}") }).collect();
        let disposition = format!("{}; filename*=UTF-8''{filename}", if mime.starts_with("image/") || ["application/pdf", "text/plain"].contains(&mime) { "inline" } else { "attachment" });
        Ok((
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "sandbox; default-src 'none'; style-src 'unsafe-inline'",
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (
                    header::CONTENT_DISPOSITION,
                    disposition.as_str(),
                ),
            ],
            content,
        )
            .into_response())
    })
    .await
}

#[derive(Deserialize)]
struct AttachmentUpload { version: String, upload: attachments::Upload }
async fn upload(State(state): State<Arc<Service>>, RoutePath(id): RoutePath<String>, Json(request): Json<AttachmentUpload>) -> ApiResult<Json<attachments::ImportedAttachment>> {
    with_store(state, move |store| {
        versioned_note(store, &id, &request.version)?;
        attachments::import(store, &id, request.upload).map(Json).map_err(|_| ApiError::bad_request("Invalid attachment name or file exceeds 64 MiB"))
    }).await
}

#[cfg(test)]
mod tests;
