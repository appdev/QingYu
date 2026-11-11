//! Browser operations share the desktop transactions, never accept an entire client library.
use super::*;

pub(super) fn router(state: Arc<Service>) -> Router<Arc<Service>> {
    Router::new()
        .route("/web/library", get(snapshot))
        .route("/web/commands", axum::routing::post(change))
        .route("/web/assets/{id}", get(asset))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(state, browser_only))
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
        let note = store
            .data
            .notes
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(ApiError::not_found)?;
        if query.path.len() > 4096
            || query.path.contains(['\\', ':', '\0'])
            || Path::new(&query.path).is_absolute()
        {
            return Err(ApiError::not_found());
        }
        let mut pieces: Vec<String> = Path::new(&note.path)
            .parent()
            .unwrap_or(Path::new(""))
            .components()
            .map(|p| p.as_os_str().to_string_lossy().into_owned())
            .collect();
        for part in query.path.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if pieces.pop().is_none() {
                        return Err(ApiError::not_found());
                    }
                }
                value => pieces.push(value.into()),
            }
        }
        let path = pieces.join("/");
        if !assets::valid_path(&path) || pieces.iter().any(|part| part.starts_with('.')) {
            return Err(ApiError::not_found());
        }
        let path = checked(&store.root, &path).map_err(|_| ApiError::not_found())?;
        let meta = fs::metadata(&path).map_err(|_| ApiError::not_found())?;
        if !meta.is_file() || meta.len() > 64 * 1024 * 1024 {
            return Err(ApiError::not_found());
        }
        let content = fs::read(&path).map_err(|_| ApiError::not_found())?;
        let mime = match path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };
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
                    if mime.starts_with("image/") {
                        "inline"
                    } else {
                        "attachment"
                    },
                ),
            ],
            content,
        )
            .into_response())
    })
    .await
}

#[cfg(test)]
mod tests;
