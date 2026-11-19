//! Browser operations share the desktop transactions, never accept an entire client library.
use super::*;
use crate::file_library::storage_usage::{StorageUsage, UsageTarget};

pub(super) fn router(state: Arc<Service>) -> Router<Arc<Service>> {
    Router::new()
        .route("/web/library", get(snapshot))
        .route("/web/notes/{id}/versions", get(versions))
        .route("/web/notes/{id}/versions/{revision}", get(version_source))
        .route("/web/storage-usage", get(storage_usage))
        .route("/web/commands", axum::routing::post(change))
        .route(
            "/web/import",
            axum::routing::post(import).layer(DefaultBodyLimit::max(MAX_IMPORT * 2)),
        )
        .route(
            "/web/link-preview",
            axum::routing::post(link_preview).layer(DefaultBodyLimit::max(12 * 1024)),
        )
        .route(
            "/web/assets/{id}",
            get(asset)
                .post(upload)
                .layer(DefaultBodyLimit::max(assets::MAX_FILE * 4 + 65536)),
        )
        .merge(account::routes())
        .merge(server_sync::routes())
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(state, browser_only))
}

/// 一次导入的总大小上限（解码后），按 base64 上传时请求体约为它的 4/3。
const MAX_IMPORT: usize = 32 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportFile {
    /// 文件名；导入文件夹时是相对路径，例如「旅行/京都.md」
    path: String,
    data: String,
}

/// 导入本机文件，和 App 的「导入文件…」「导入文件夹…」相同：先写进临时目录，再交给 DiskLibrary::import。
async fn import(
    State(state): State<Arc<Service>>,
    body: std::result::Result<Json<Vec<ImportFile>>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let files = payload(body)?;
    let staging = tempfile::tempdir().map_err(|_| ApiError::internal())?;
    let mut total = 0;
    for file in files {
        let parts: Vec<&str> = file.path.split('/').collect();
        if parts.iter().any(|part| {
            part.is_empty() || part.starts_with('.') || part.chars().any(|c| c.is_control() || c == '\\')
        }) || parts.len() > 32
        {
            return Err(ApiError::bad_request("Invalid import file name"));
        }
        let bytes = STANDARD
            .decode(file.data)
            .map_err(|_| ApiError::bad_request("Invalid import file content"))?;
        total += bytes.len();
        if total > MAX_IMPORT {
            return Err(ApiError::bad_request("Import exceeds 32 MiB"));
        }
        let target = parts.iter().fold(staging.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(target.parent().ok_or_else(ApiError::internal)?).map_err(|_| ApiError::internal())?;
        fs::write(&target, bytes).map_err(|_| ApiError::internal())?;
    }
    let root = staging.path().to_path_buf();
    let result = with_store(state, move |store| {
        store.import(&[root]).map_err(library_error)?;
        view(store)
    })
    .await;
    drop(staging);
    result
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
            "Sign in to use the editor".into(),
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
        "trashedFolders": crate::file_library::folders::trashed_folders(&store.root),
        "versions": versions, "version": library_version(store)? }),
    ))
}

async fn snapshot(State(state): State<Arc<Service>>) -> ApiResult<Json<serde_json::Value>> {
    with_store(state, |store| view(store)).await
}

async fn versions(
    State(state): State<Arc<Service>>,
    RoutePath(id): RoutePath<String>,
) -> ApiResult<Json<Vec<crate::file_library::history::NoteVersion>>> {
    with_store(state, move |store| {
        store
            .note_versions(&id)
            .map(Json)
            .map_err(|_| ApiError::not_found())
    })
    .await
}

async fn version_source(
    State(state): State<Arc<Service>>,
    RoutePath((id, revision)): RoutePath<(String, String)>,
) -> ApiResult<Json<String>> {
    with_store(state, move |store| {
        store
            .note_version_source(&id, &revision)
            .map(Json)
            .map_err(|_| ApiError::not_found())
    })
    .await
}

async fn storage_usage(State(state): State<Arc<Service>>) -> ApiResult<Json<StorageUsage>> {
    let target = {
        let store = state.store.lock().map_err(|_| ApiError::internal())?;
        UsageTarget::new(&store)
    };
    tokio::task::spawn_blocking(move || {
        target.measure().map(Json).map_err(|_| ApiError::internal())
    })
    .await
    .map_err(|_| ApiError::internal())?
}

/// 和前端 validShare 一致：链接只接受 http(s)，长度不超过 2048。
fn valid_share(share: &NoteShare) -> bool {
    share.url.as_deref().is_none_or(|url| {
        url.len() <= 2048
            && (url.starts_with("https://") || url.starts_with("http://"))
            && !url.chars().any(char::is_whitespace)
    })
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
        #[serde(default)]
        share: Option<NoteShare>,
    },
    DuplicateNote {
        id: String,
        version: String,
        new_id: String,
    },
    RestoreNoteVersion {
        id: String,
        version: String,
        revision: String,
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
    /// 和 App 移动端相同的文件夹操作：复制、移到指定文件夹、移到笔记库废纸篓、从废纸篓恢复
    FolderAction {
        action: String,
        id: Option<String>,
        version: String,
        #[serde(default)]
        value: String,
    },
    PurgeNotes {
        ids: Vec<String>,
        version: String,
    },
}

fn library_changed(store: &DiskLibrary, version: &str) -> ApiResult<()> {
    if version != library_version(store)? {
        return Err(ApiError(
            StatusCode::PRECONDITION_FAILED,
            "version_conflict",
            "The library changed; reload before changing it".into(),
        ));
    }
    Ok(())
}

/// 带上笔记库给出的原因，网页和 App 显示同样的提示
fn library_error(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::CONFLICT, "library_conflict", error.to_string().into())
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
            "The document changed; keep your draft and reload".into(),
        ));
    }
    if note.read_only {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "read_only_document",
            "This document cannot be modified".into(),
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
                    share: None,
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
                share,
            } => {
                versioned_note(store, &id, &version)?;
                validate_input(title.as_deref(), &source)?;
                if share.as_ref().is_some_and(|share| !valid_share(share)) {
                    return Err(ApiError::bad_request("Invalid share link"));
                }
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
                note.share = share;
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
                    share: None,
                });
                commit(store, next, &format!("duplicate:{id}"))?;
            }
            Command::RestoreNoteVersion {
                id,
                version,
                revision,
            } => {
                let current = versioned_note(store, &id, &version)?.source.clone();
                let root = store.root.to_string_lossy().into_owned();
                store
                    .restore_note_version(&root, &id, &revision, &current)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "history_restore_failed",
                            "The version could not be restored".into(),
                        )
                    })?;
            }
            Command::CreateFolder { parent_id, name } => {
                store
                    .folder_change("create", &parent_id, &name, None)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "folder_conflict",
                            "The folder cannot be created".into(),
                        )
                    })?;
            }
            Command::RenameFolder { id, version, name } => {
                if version != library_version(store)? {
                    return Err(ApiError(
                        StatusCode::PRECONDITION_FAILED,
                        "version_conflict",
                        "The library changed; reload before moving folders".into(),
                    ));
                }
                store
                    .folder_change("rename", &id, &name, None)
                    .map_err(|_| {
                        ApiError(
                            StatusCode::CONFLICT,
                            "folder_conflict",
                            "The folder cannot be renamed".into(),
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
                        "The library changed; reload before moving folders".into(),
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
                            "The folder cannot be moved".into(),
                        )
                    })?;
            }
            Command::FolderAction {
                action,
                id,
                version,
                value,
            } => {
                if !matches!(
                    action.as_str(),
                    "duplicate" | "move-to-folder" | "trash-local" | "restore-folder"
                ) {
                    return Err(ApiError::bad_request("Unsupported folder action"));
                }
                library_changed(store, &version)?;
                store
                    .folder_change(&action, id.as_deref().unwrap_or("default"), &value, None)
                    .map_err(library_error)?;
            }
            Command::PurgeNotes { ids, version } => {
                library_changed(store, &version)?;
                store.purge_notes(&ids).map_err(library_error)?;
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
        let path =
            attachments::resolve(store, &id, &query.path).map_err(|_| ApiError::not_found())?;
        let content = attachments::read(&path).map_err(|_| ApiError::not_found())?;
        let mime = attachments::mime(&path);
        let filename: String = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"-_.".contains(byte) {
                    (*byte as char).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        let disposition = format!(
            "{}; filename*=UTF-8''{filename}",
            if mime.starts_with("image/") || ["application/pdf", "text/plain"].contains(&mime) {
                "inline"
            } else {
                "attachment"
            }
        );
        Ok((
            [
                (header::CONTENT_TYPE, mime),
                (
                    header::CONTENT_SECURITY_POLICY,
                    "sandbox; default-src 'none'; style-src 'unsafe-inline'",
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CONTENT_DISPOSITION, disposition.as_str()),
            ],
            content,
        )
            .into_response())
    })
    .await
}

#[derive(Deserialize)]
struct AttachmentUpload {
    version: String,
    upload: attachments::Upload,
}
async fn upload(
    State(state): State<Arc<Service>>,
    RoutePath(id): RoutePath<String>,
    Json(request): Json<AttachmentUpload>,
) -> ApiResult<Json<attachments::ImportedAttachment>> {
    with_store(state, move |store| {
        versioned_note(store, &id, &request.version)?;
        attachments::import(store, &id, request.upload)
            .map(Json)
            .map_err(|_| ApiError::bad_request("Invalid attachment name or file exceeds 64 MiB"))
    })
    .await
}

#[cfg(test)]
mod tests;
