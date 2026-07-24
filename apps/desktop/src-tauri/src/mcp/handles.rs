use std::{
    fmt, io,
    path::{Component, Path, PathBuf},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use super::workspaces::{ResolvedWorkspace, WorkspaceError, WorkspaceRegistry};

const HANDLE_VERSION: u8 = 2;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HandleKind {
    Document,
    Folder,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct HandlePayload {
    version: u8,
    kind: HandleKind,
    workspace_id: Uuid,
    workspace_generation: u64,
    relative_path: String,
}

#[derive(Clone)]
pub(crate) struct VerifiedDocumentHandle {
    workspace: ResolvedWorkspace,
    relative_path: PathBuf,
}

impl VerifiedDocumentHandle {
    pub(crate) fn workspace_id(&self) -> Uuid {
        self.workspace.workspace_id
    }

    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub(crate) fn workspace(&self) -> &ResolvedWorkspace {
        &self.workspace
    }

    pub(crate) fn open_file(&self) -> Result<File, HandleError> {
        self.workspace
            .revalidate_authority()
            .map_err(HandleError::from_workspace)?;
        let parent = self.relative_path.parent().unwrap_or_else(|| Path::new(""));
        let file_name = self
            .relative_path
            .file_name()
            .ok_or_else(HandleError::invalid)?;
        let directory = open_folder_nofollow(&self.workspace.root, parent)?;
        let addressed = directory
            .symlink_metadata(file_name)
            .map_err(|_| HandleError::unavailable())?;
        if addressed.file_type().is_symlink() || !addressed.is_file() {
            return Err(HandleError::boundary());
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
        let file = directory
            .open_with(file_name, &options)
            .map_err(|_| HandleError::unavailable())?;
        let metadata = file.metadata().map_err(|_| HandleError::unavailable())?;
        if !metadata.is_file()
            || addressed.dev() != metadata.dev()
            || addressed.ino() != metadata.ino()
        {
            return Err(HandleError::boundary());
        }
        Ok(file)
    }

    pub(crate) fn open_parent_dir(&self) -> Result<Dir, HandleError> {
        self.workspace
            .revalidate_authority()
            .map_err(HandleError::from_workspace)?;
        open_folder_nofollow(
            &self.workspace.root,
            self.relative_path.parent().unwrap_or_else(|| Path::new("")),
        )
    }
}

#[derive(Clone)]
pub(crate) struct VerifiedFolderHandle {
    workspace: ResolvedWorkspace,
    relative_path: PathBuf,
}

impl VerifiedFolderHandle {
    pub(crate) fn workspace_id(&self) -> Uuid {
        self.workspace.workspace_id
    }

    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    pub(crate) fn workspace(&self) -> &ResolvedWorkspace {
        &self.workspace
    }

    pub(crate) fn open_dir(&self) -> Result<Dir, HandleError> {
        self.workspace
            .revalidate_authority()
            .map_err(HandleError::from_workspace)?;
        open_folder_nofollow(&self.workspace.root, &self.relative_path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HandleError {
    pub(crate) code: &'static str,
    message: &'static str,
}

impl HandleError {
    fn invalid() -> Self {
        Self {
            code: "invalid_handle",
            message: "The MCP object identifier is invalid.",
        }
    }

    fn boundary() -> Self {
        Self {
            code: "path_boundary_violation",
            message: "The MCP object is outside its authorized directory.",
        }
    }

    fn protected() -> Self {
        Self {
            code: "protected_path",
            message: "The requested MCP object is protected.",
        }
    }

    fn unavailable() -> Self {
        Self {
            code: "document_not_found",
            message: "The requested MCP object is unavailable.",
        }
    }

    fn stale() -> Self {
        Self {
            code: "mcp-handle-stale",
            message: "The MCP object identifier belongs to an older primary workspace.",
        }
    }

    fn from_workspace(error: WorkspaceError) -> Self {
        match error.code {
            "workspace_not_authorized" => Self {
                code: "workspace_not_authorized",
                message: "The MCP workspace is not authorized.",
            },
            "workspace_unavailable" => Self {
                code: "workspace_unavailable",
                message: "The authorized MCP workspace is unavailable.",
            },
            "mcp-workspace-unavailable" => Self {
                code: "mcp-workspace-unavailable",
                message: "A valid primary notes workspace is required for MCP document tools.",
            },
            "mcp-handle-stale" => Self::stale(),
            _ => Self::invalid(),
        }
    }
}

impl fmt::Display for HandleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HandleError {}

#[derive(Clone)]
pub(crate) struct HandleSigner {
    key: [u8; 32],
}

impl HandleSigner {
    pub(crate) fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub(crate) fn derive_key(&self, context: &[u8]) -> [u8; 32] {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("HMAC accepts the fixed-size QingYu signing key");
        mac.update(context);
        mac.finalize().into_bytes().into()
    }

    pub(crate) fn issue_document(
        &self,
        workspace_id: Uuid,
        workspace_generation: u64,
        relative_path: &str,
    ) -> Result<String, HandleError> {
        let path = validate_relative_path(relative_path, false)?;
        if !is_markdown_path(&path) {
            return Err(HandleError::invalid());
        }
        self.issue(
            HandleKind::Document,
            workspace_id,
            workspace_generation,
            relative_path,
        )
    }

    pub(crate) fn issue_folder(
        &self,
        workspace_id: Uuid,
        workspace_generation: u64,
        relative_path: &str,
    ) -> Result<String, HandleError> {
        validate_relative_path(relative_path, true)?;
        self.issue(
            HandleKind::Folder,
            workspace_id,
            workspace_generation,
            relative_path,
        )
    }

    fn issue(
        &self,
        kind: HandleKind,
        workspace_id: Uuid,
        workspace_generation: u64,
        relative_path: &str,
    ) -> Result<String, HandleError> {
        let payload = HandlePayload {
            version: HANDLE_VERSION,
            kind,
            workspace_id,
            workspace_generation,
            relative_path: relative_path.to_string(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| HandleError::invalid())?;
        let signature = self.sign(&bytes)?;
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(bytes),
            URL_SAFE_NO_PAD.encode(signature)
        ))
    }

    pub(crate) fn verify_document(
        &self,
        handle: &str,
        registry: &WorkspaceRegistry,
    ) -> Result<VerifiedDocumentHandle, HandleError> {
        let payload = self.verify_payload(handle, HandleKind::Document)?;
        let workspace = registry
            .resolve_at_generation(payload.workspace_id, payload.workspace_generation)
            .map_err(HandleError::from_workspace)?;
        let relative_path = validate_relative_path(&payload.relative_path, false)?;
        if !is_markdown_path(&relative_path) {
            return Err(HandleError::invalid());
        }
        workspace
            .revalidate_authority()
            .map_err(HandleError::from_workspace)?;
        verify_document_path(&workspace.root, &relative_path)?;
        Ok(VerifiedDocumentHandle {
            workspace,
            relative_path,
        })
    }

    pub(crate) fn verify_document_in_workspace(
        &self,
        handle: &str,
        workspace_id: Uuid,
        registry: &WorkspaceRegistry,
    ) -> Result<VerifiedDocumentHandle, HandleError> {
        let verified = self.verify_document(handle, registry)?;
        if verified.workspace_id() != workspace_id {
            return Err(HandleError::invalid());
        }
        Ok(verified)
    }

    pub(crate) fn verify_folder(
        &self,
        handle: &str,
        registry: &WorkspaceRegistry,
    ) -> Result<VerifiedFolderHandle, HandleError> {
        let payload = self.verify_payload(handle, HandleKind::Folder)?;
        let workspace = registry
            .resolve_at_generation(payload.workspace_id, payload.workspace_generation)
            .map_err(HandleError::from_workspace)?;
        let relative_path = validate_relative_path(&payload.relative_path, true)?;
        workspace
            .revalidate_authority()
            .map_err(HandleError::from_workspace)?;
        verify_folder_path(&workspace.root, &relative_path)?;
        Ok(VerifiedFolderHandle {
            workspace,
            relative_path,
        })
    }

    fn verify_payload(
        &self,
        handle: &str,
        expected_kind: HandleKind,
    ) -> Result<HandlePayload, HandleError> {
        let mut parts = handle.split('.');
        let payload = parts.next().ok_or_else(HandleError::invalid)?;
        let signature = parts.next().ok_or_else(HandleError::invalid)?;
        if parts.next().is_some() || payload.is_empty() || signature.is_empty() {
            return Err(HandleError::invalid());
        }
        let payload = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| HandleError::invalid())?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| HandleError::invalid())?;
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| HandleError::invalid())?;
        mac.update(&payload);
        mac.verify_slice(&signature)
            .map_err(|_| HandleError::invalid())?;
        let payload = serde_json::from_slice::<HandlePayload>(&payload)
            .map_err(|_| HandleError::invalid())?;
        if payload.version != HANDLE_VERSION || payload.kind != expected_kind {
            return Err(HandleError::invalid());
        }
        Ok(payload)
    }

    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, HandleError> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).map_err(|_| HandleError::invalid())?;
        mac.update(payload);
        Ok(mac.finalize().into_bytes().to_vec())
    }
}

fn validate_relative_path(path: &str, allow_root: bool) -> Result<PathBuf, HandleError> {
    if path.is_empty() {
        return if allow_root {
            Ok(PathBuf::new())
        } else {
            Err(HandleError::invalid())
        };
    }
    if path.len() > 4096
        || path.contains('\0')
        || path.contains('\\')
        || path.starts_with('/')
        || path.starts_with("//")
        || has_windows_drive_prefix(path)
        || contains_encoded_path_syntax(path)
    {
        return Err(HandleError::boundary());
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(HandleError::boundary());
    }

    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(HandleError::boundary());
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        let Component::Normal(segment) = component else {
            return Err(HandleError::boundary());
        };
        let segment_text = segment.to_str().ok_or_else(HandleError::invalid)?;
        if segment_text.is_empty() || is_protected_segment(segment_text) {
            return Err(HandleError::protected());
        }
        normalized.push(segment);
    }
    if normalized.as_os_str().is_empty() && !allow_root {
        return Err(HandleError::invalid());
    }
    Ok(normalized)
}

fn has_windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn contains_encoded_path_syntax(path: &str) -> bool {
    let lowercase = path.to_ascii_lowercase();
    ["%2e", "%2f", "%5c", "%25"]
        .iter()
        .any(|encoded| lowercase.contains(encoded))
}

fn is_protected_segment(segment: &str) -> bool {
    let segment = segment.to_ascii_lowercase();
    matches!(
        segment.as_str(),
        ".qingyu"
            | ".markra-sync"
            | ".git"
            | ".codex"
            | ".obsidian"
            | "node_modules"
            | "target"
            | "build"
            | "dist"
            | ".markraignore"
    )
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        })
}

fn verify_folder_path(root: &Dir, path: &Path) -> Result<(), HandleError> {
    open_folder_nofollow(root, path).map(|_| ())
}

fn verify_document_path(root: &Dir, path: &Path) -> Result<(), HandleError> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let file_name = path.file_name().ok_or_else(HandleError::invalid)?;
    let directory = open_folder_nofollow(root, parent)?;
    let metadata = directory
        .symlink_metadata(file_name)
        .map_err(|_| HandleError::unavailable())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HandleError::boundary());
    }
    Ok(())
}

fn open_folder_nofollow(root: &Dir, path: &Path) -> Result<Dir, HandleError> {
    let mut current = root.try_clone().map_err(|_| HandleError::unavailable())?;
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(HandleError::boundary());
        };
        match current.symlink_metadata(segment) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(HandleError::boundary());
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(HandleError::unavailable());
            }
            Err(_) => return Err(HandleError::boundary()),
        }
        current = current
            .open_dir_nofollow(segment)
            .map_err(|_| HandleError::boundary())?;
    }
    Ok(current)
}
