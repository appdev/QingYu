use std::{
    io::{self, Read as _},
    path::Path,
    sync::Arc,
};

use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
};
use cap_fs_ext::{DirExt as _, MetadataExt};
use cap_std::fs::{Dir, Metadata};

use super::{
    api_error, is_api_namespace_path, is_media_namespace_path, routes, ApiState,
    InvalidServerWebAssets,
};
use crate::{
    contract::ErrorCode,
    storage::{nonfollowing_read_options, open_canonical_directory_nofollow},
};

const INDEX_NAME: &str = "index.html";
const MAX_WEB_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: blob: https:; font-src 'self' data:; object-src 'none'; \
    base-uri 'none'; frame-ancestors 'none'";

#[derive(Clone)]
pub(super) struct ServerWebAssets {
    root: Arc<Dir>,
    index: Bytes,
}

impl ServerWebAssets {
    pub(super) fn open(root: &Path) -> Result<Self, InvalidServerWebAssets> {
        let root = open_canonical_directory_nofollow(root).map_err(|_| InvalidServerWebAssets)?;
        let metadata = root.dir_metadata().map_err(|_| InvalidServerWebAssets)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(InvalidServerWebAssets);
        }
        let root = Arc::new(root);
        let index = read_file(&root, &[INDEX_NAME]).map_err(|_| InvalidServerWebAssets)?;
        Ok(Self {
            root,
            index: Bytes::from(index),
        })
    }

    fn read(&self, path: &str) -> Result<WebAsset, WebAssetError> {
        let Some(components) = safe_components(path)? else {
            return Ok(WebAsset::Index(self.index.clone()));
        };
        if components.as_slice() == [INDEX_NAME] {
            return Ok(WebAsset::Index(self.index.clone()));
        }
        match read_file(&self.root, &components) {
            Ok(bytes) => Ok(WebAsset::File {
                bytes,
                content_type: content_type(components.last().expect("asset path is non-empty")),
            }),
            Err(WebAssetReadError::Missing | WebAssetReadError::Directory) => {
                Ok(WebAsset::Index(self.index.clone()))
            }
            Err(WebAssetReadError::Unsafe) => Err(WebAssetError),
        }
    }
}

enum WebAsset {
    Index(Bytes),
    File {
        bytes: Vec<u8>,
        content_type: &'static str,
    },
}

impl WebAsset {
    fn into_parts(self) -> (Body, usize, &'static str) {
        match self {
            Self::Index(bytes) => {
                let length = bytes.len();
                (Body::from(bytes), length, "text/html; charset=utf-8")
            }
            Self::File {
                bytes,
                content_type,
            } => {
                let length = bytes.len();
                (Body::from(bytes), length, content_type)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct WebAssetError;

#[derive(Clone, Copy)]
enum WebAssetReadError {
    Missing,
    Directory,
    Unsafe,
}

pub(super) async fn fallback(State(state): State<ApiState>, request: Request) -> Response {
    if state.web.is_none() {
        return routes::not_found(request).await;
    }
    if is_api_namespace_path(request.uri().path()) || is_media_namespace_path(request.uri().path())
    {
        return api_error(ErrorCode::InvalidRequest, None);
    }
    let method = request.method().clone();
    if method != Method::GET && method != Method::HEAD {
        let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        decorate_static_response(&mut response);
        return response;
    }
    let path = request.uri().path().to_owned();
    let assets = state.web.expect("Web fallback requires Web assets");
    let asset = tokio::task::spawn_blocking(move || assets.read(&path)).await;
    let Ok(Ok(asset)) = asset else {
        let mut response = StatusCode::NOT_FOUND.into_response();
        decorate_static_response(&mut response);
        return response;
    };
    let (body, length, content_type) = asset.into_parts();
    let mut response = if method == Method::HEAD {
        Body::empty().into_response()
    } else {
        body.into_response()
    };
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).expect("asset length is a valid header"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    decorate_static_response(&mut response);
    response
}

fn safe_components(path: &str) -> Result<Option<Vec<&str>>, WebAssetError> {
    if path == "/" {
        return Ok(None);
    }
    if !path.starts_with('/') || path.contains('%') || path.contains('\\') || path.contains('\0') {
        return Err(WebAssetError);
    }
    let relative = path[1..].strip_suffix('/').unwrap_or(&path[1..]);
    if relative.is_empty() {
        return Ok(None);
    }
    let components = relative.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        component.is_empty()
            || *component == "."
            || *component == ".."
            || component.starts_with('.')
    }) {
        return Err(WebAssetError);
    }
    Ok(Some(components))
}

fn read_file(root: &Dir, components: &[&str]) -> Result<Vec<u8>, WebAssetReadError> {
    let (name, parents) = components.split_last().ok_or(WebAssetReadError::Unsafe)?;
    let mut directory = root.try_clone().map_err(|_| WebAssetReadError::Unsafe)?;
    for parent in parents {
        let metadata = directory
            .symlink_metadata(parent)
            .map_err(classify_io_error)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(WebAssetReadError::Unsafe);
        }
        directory = directory
            .open_dir_nofollow(parent)
            .map_err(|_| WebAssetReadError::Unsafe)?;
    }
    let addressed = directory
        .symlink_metadata(name)
        .map_err(classify_io_error)?;
    if addressed.file_type().is_symlink() {
        return Err(WebAssetReadError::Unsafe);
    }
    if addressed.is_dir() {
        return Err(WebAssetReadError::Directory);
    }
    if !is_safe_regular_file(&addressed) {
        return Err(WebAssetReadError::Unsafe);
    }
    let mut file = directory
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| WebAssetReadError::Unsafe)?;
    let retained = file.metadata().map_err(|_| WebAssetReadError::Unsafe)?;
    if !same_stable_file(&addressed, &retained) {
        return Err(WebAssetReadError::Unsafe);
    }
    let capacity = usize::try_from(retained.len()).map_err(|_| WebAssetReadError::Unsafe)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)
        .map_err(|_| WebAssetReadError::Unsafe)?;
    if bytes.len() != capacity {
        return Err(WebAssetReadError::Unsafe);
    }
    let after = file.metadata().map_err(|_| WebAssetReadError::Unsafe)?;
    let named = directory
        .symlink_metadata(name)
        .map_err(classify_io_error)?;
    if !same_stable_file(&retained, &after) || !same_stable_file(&after, &named) {
        return Err(WebAssetReadError::Unsafe);
    }
    Ok(bytes)
}

fn is_safe_regular_file(metadata: &Metadata) -> bool {
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && MetadataExt::nlink(metadata) == 1
        && metadata.len() <= MAX_WEB_ASSET_BYTES
}

fn same_stable_file(left: &Metadata, right: &Metadata) -> bool {
    is_safe_regular_file(left)
        && is_safe_regular_file(right)
        && MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
        && left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
}

fn classify_io_error(error: io::Error) -> WebAssetReadError {
    if error.kind() == io::ErrorKind::NotFound {
        WebAssetReadError::Missing
    } else {
        WebAssetReadError::Unsafe
    }
}

fn content_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_stem, extension)| extension)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpeg" | "jpg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "otf" => "font/otf",
        "png" => "image/png",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn decorate_static_response(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
}
