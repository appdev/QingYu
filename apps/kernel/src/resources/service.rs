use std::{
    fmt,
    io::{self, Read as _, Seek as _, SeekFrom},
    sync::{Arc, Weak},
};

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::{Dir, File, Metadata};
use sha2::{Digest as _, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    contract::{
        DocumentEntryDto, DocumentKind, DocumentName, ResourceEntryDto, ResourceId, ResourceKind,
        ResourceName, Revision, Rfc3339Utc, SafeUnsignedInteger, WorkspaceDto, WorkspaceReadiness,
        WorkspaceRelativePath,
    },
    documents::{service::directory_revision_for_capability, DocumentIgnorePort},
    runtime::{ActiveWorkspaceSnapshot, KernelRuntime},
    storage::nonfollowing_read_options,
};

use super::{policy::protected_resource_component, ResourceServiceError};

const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const MAGIC_BYTES: usize = 12;

pub struct WorkspaceResourceService {
    runtime: Weak<KernelRuntime>,
    ignore: Arc<dyn DocumentIgnorePort>,
}

impl WorkspaceResourceService {
    pub fn new(runtime: &Arc<KernelRuntime>, ignore: Arc<dyn DocumentIgnorePort>) -> Self {
        Self {
            runtime: Arc::downgrade(runtime),
            ignore,
        }
    }

    /// Lists one directory level from the retained active workspace capability.
    /// Documents and directories remain document DTOs; binary entries use the
    /// separate resource identity and DTO contract.
    pub fn list_inventory(
        &self,
        parent: &WorkspaceRelativePath,
    ) -> Result<Vec<WorkspaceInventoryEntry>, ResourceServiceError> {
        let context = self.context()?;
        let directory = open_directory(&context.root, parent)?;
        let before = trusted_directory_metadata(&directory)?;
        let names = ordinary_entry_names(&directory)?;
        let mut entries = Vec::with_capacity(names.len());
        for name in &names {
            if let Some(entry) =
                inspect_inventory_entry(&context, self.ignore.as_ref(), &directory, parent, name)?
            {
                entries.push(entry);
            }
        }
        if ordinary_entry_names(&directory)? != names {
            return Err(ResourceServiceError::unsafe_target());
        }
        let after = trusted_directory_metadata(&directory)?;
        if !same_file(&before, &after) {
            return Err(ResourceServiceError::unsafe_target());
        }
        context
            .snapshot
            .authority()
            .verify_held_directory()
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(entries)
    }

    /// Opens a signed resource identity as a retained, synchronous reader.
    /// Future transports must consume the declared length and then call
    /// [`RetainedResource::verify_complete`]; observing `Ok(0)` alone does not
    /// authenticate the completed stream.
    pub fn open_resource(
        &self,
        id: &ResourceId,
        expected_kind: ResourceKind,
    ) -> Result<RetainedResource, ResourceServiceError> {
        let context = self.context()?;
        let path = context
            .runtime
            .wire_identity_key()
            .verify_resource_id(
                id,
                context.workspace().id,
                &context.workspace().generation,
                expected_kind,
            )
            .map_err(|_| ResourceServiceError::not_found())?;
        let (parent_path, name) = parent_and_name(&path)?;
        if protected_resource_component(&name) {
            return Err(ResourceServiceError::invalid_path());
        }
        if self.ignore.is_ignored(&path, DocumentKind::File) {
            return Err(ResourceServiceError::not_found());
        }
        let resource_name =
            ResourceName::parse(&name).map_err(|_| ResourceServiceError::invalid_path())?;
        let parent = open_directory(&context.root, &parent_path)?;
        let addressed = parent.symlink_metadata(&name).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                ResourceServiceError::not_found()
            } else {
                ResourceServiceError::unavailable()
            }
        })?;
        let inspected = inspect_regular_file(&parent, &name, &addressed)?;
        if markdown_name(&name) {
            return Err(ResourceServiceError::wrong_kind());
        }
        let classification = classify_resource(&name, &inspected.magic);
        if classification.kind != expected_kind {
            return Err(ResourceServiceError::wrong_kind());
        }
        let entry = ResourceEntryDto {
            id: id.clone(),
            path,
            parent: parent_path,
            name: resource_name,
            kind: classification.kind,
            size_bytes: safe_size(inspected.metadata.len())?,
            modified_at: modified_utc(&inspected.metadata)?,
            revision: inspected.revision,
            media_type: classification.media_type.to_string(),
            previewable: classification.previewable,
        };
        let remaining = inspected.metadata.len();
        Ok(RetainedResource {
            snapshot: context.snapshot,
            parent,
            file: inspected.file,
            entry,
            expected: inspected.metadata,
            remaining,
            stream_digest: Sha256::new(),
            verified_complete: false,
        })
    }

    fn context(&self) -> Result<ResourceContext, ResourceServiceError> {
        let runtime = self
            .runtime
            .upgrade()
            .ok_or_else(ResourceServiceError::unavailable)?;
        runtime
            .verify_instance_lock()
            .map_err(|_| ResourceServiceError::unavailable())?;
        let snapshot = runtime
            .active_workspace_snapshot()
            .map_err(|_| ResourceServiceError::unavailable())?;
        if snapshot.workspace().readiness != WorkspaceReadiness::Ready {
            return Err(ResourceServiceError::unavailable());
        }
        let root = snapshot
            .authority()
            .root()
            .try_clone_dir()
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(ResourceContext {
            runtime,
            snapshot,
            root,
        })
    }
}

impl fmt::Debug for WorkspaceResourceService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WorkspaceResourceService { runtime: weak }")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceInventoryEntry {
    Document(DocumentEntryDto),
    Resource(ResourceEntryDto),
}

impl WorkspaceInventoryEntry {
    pub const fn path(&self) -> &WorkspaceRelativePath {
        match self {
            Self::Document(entry) => &entry.path,
            Self::Resource(entry) => &entry.path,
        }
    }
}

struct ResourceContext {
    runtime: Arc<KernelRuntime>,
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    root: Dir,
}

impl ResourceContext {
    fn workspace(&self) -> &WorkspaceDto {
        self.snapshot.workspace()
    }
}

fn inspect_inventory_entry(
    context: &ResourceContext,
    ignore: &dyn DocumentIgnorePort,
    directory: &Dir,
    parent: &WorkspaceRelativePath,
    name: &str,
) -> Result<Option<WorkspaceInventoryEntry>, ResourceServiceError> {
    let addressed = directory
        .symlink_metadata(name)
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    if addressed.file_type().is_symlink() {
        return Err(ResourceServiceError::unsafe_target());
    }
    let path = join_relative(parent, name)?;
    let ignore_kind = if addressed.is_dir() {
        DocumentKind::Directory
    } else if addressed.is_file() {
        DocumentKind::File
    } else {
        return Err(ResourceServiceError::unsafe_target());
    };
    if ignore.is_ignored(&path, ignore_kind) {
        return Ok(None);
    }
    if addressed.is_dir() {
        let child = directory
            .open_dir_nofollow(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let retained = trusted_directory_metadata(&child)?;
        if !same_file(&addressed, &retained) {
            return Err(ResourceServiceError::unsafe_target());
        }
        let revision = directory_revision_for_capability(&child)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        let after = trusted_directory_metadata(&child)?;
        let named = directory
            .symlink_metadata(name)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
        if !named.is_dir()
            || named.file_type().is_symlink()
            || !same_file(&retained, &after)
            || !same_file(&retained, &named)
        {
            return Err(ResourceServiceError::unsafe_target());
        }
        let entry = document_entry(
            context,
            parent,
            path,
            name,
            DocumentKind::Directory,
            &after,
            revision,
        )?;
        return Ok(Some(WorkspaceInventoryEntry::Document(entry)));
    }
    if !addressed.is_file() {
        return Err(ResourceServiceError::unsafe_target());
    }
    let inspected = inspect_regular_file(directory, name, &addressed)?;
    if markdown_name(name) {
        let entry = document_entry(
            context,
            parent,
            path,
            name,
            DocumentKind::File,
            &inspected.metadata,
            inspected.revision,
        )?;
        Ok(Some(WorkspaceInventoryEntry::Document(entry)))
    } else {
        let classification = classify_resource(name, &inspected.magic);
        let resource_name =
            ResourceName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?;
        let id = context
            .runtime
            .wire_identity_key()
            .issue_resource_id(
                context.workspace().id,
                &context.workspace().generation,
                classification.kind,
                &path,
            )
            .map_err(|_| ResourceServiceError::unavailable())?;
        Ok(Some(WorkspaceInventoryEntry::Resource(ResourceEntryDto {
            id,
            path,
            parent: parent.clone(),
            name: resource_name,
            kind: classification.kind,
            size_bytes: safe_size(inspected.metadata.len())?,
            modified_at: modified_utc(&inspected.metadata)?,
            revision: inspected.revision,
            media_type: classification.media_type.to_string(),
            previewable: classification.previewable,
        })))
    }
}

fn document_entry(
    context: &ResourceContext,
    parent: &WorkspaceRelativePath,
    path: WorkspaceRelativePath,
    name: &str,
    kind: DocumentKind,
    metadata: &Metadata,
    revision: Revision,
) -> Result<DocumentEntryDto, ResourceServiceError> {
    let id = context
        .runtime
        .wire_identity_key()
        .issue_document_id(
            context.workspace().id,
            &context.workspace().generation,
            kind,
            &path,
        )
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(DocumentEntryDto {
        id,
        path,
        parent: parent.clone(),
        name: DocumentName::parse(name).map_err(|_| ResourceServiceError::invalid_path())?,
        kind,
        size_bytes: if kind == DocumentKind::File {
            safe_size(metadata.len())?
        } else {
            SafeUnsignedInteger::ZERO
        },
        modified_at: modified_utc(metadata)?,
        revision,
    })
}

struct InspectedFile {
    file: File,
    metadata: Metadata,
    revision: Revision,
    magic: [u8; MAGIC_BYTES],
}

fn inspect_regular_file(
    directory: &Dir,
    name: &str,
    addressed: &Metadata,
) -> Result<InspectedFile, ResourceServiceError> {
    if !trusted_regular_file(addressed) {
        return Err(ResourceServiceError::unsafe_target());
    }
    let mut file = directory
        .open_with(name, &nonfollowing_read_options())
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    let retained = file
        .metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    if !trusted_regular_file(&retained) || !same_file(addressed, &retained) {
        return Err(ResourceServiceError::unsafe_target());
    }
    let expected_modified = retained
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut magic = [0_u8; MAGIC_BYTES];
    let mut magic_len = 0;
    let mut buffer = [0_u8; STREAM_BUFFER_BYTES];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ResourceServiceError::unavailable())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(ResourceServiceError::unsafe_target)?;
        if total > retained.len() {
            return Err(ResourceServiceError::unsafe_target());
        }
        let copy = (MAGIC_BYTES - magic_len).min(read);
        magic[magic_len..magic_len + copy].copy_from_slice(&buffer[..copy]);
        magic_len += copy;
        digest.update(&buffer[..read]);
    }
    let after = file
        .metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    let named = directory
        .symlink_metadata(name)
        .map_err(|_| ResourceServiceError::unsafe_target())?;
    if !trusted_regular_file(&after)
        || !trusted_regular_file(&named)
        || !same_file(&retained, &after)
        || !same_file(&retained, &named)
        || total != retained.len()
        || after.len() != retained.len()
        || named.len() != retained.len()
        || after.modified().ok() != Some(expected_modified)
        || named.modified().ok() != Some(expected_modified)
    {
        return Err(ResourceServiceError::unsafe_target());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ResourceServiceError::unavailable())?;
    let revision = Revision::parse(format!("sha256:{:x}", digest.finalize()))
        .map_err(|_| ResourceServiceError::unavailable())?;
    Ok(InspectedFile {
        file,
        metadata: after,
        revision,
        magic,
    })
}

fn ordinary_entry_names(directory: &Dir) -> Result<Vec<String>, ResourceServiceError> {
    let mut names = Vec::new();
    for entry in directory
        .entries()
        .map_err(|_| ResourceServiceError::unavailable())?
    {
        let entry = entry.map_err(|_| ResourceServiceError::unavailable())?;
        let name = entry
            .file_name()
            .to_str()
            .map(str::to_owned)
            .ok_or_else(ResourceServiceError::invalid_path)?;
        if protected_resource_component(&name) {
            continue;
        }
        ResourceName::parse(&name).map_err(|_| ResourceServiceError::invalid_path())?;
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn open_directory(root: &Dir, path: &WorkspaceRelativePath) -> Result<Dir, ResourceServiceError> {
    let mut directory = root
        .try_clone()
        .map_err(|_| ResourceServiceError::unavailable())?;
    for component in path
        .as_str()
        .split('/')
        .filter(|component| !component.is_empty())
    {
        if protected_resource_component(component) {
            return Err(ResourceServiceError::invalid_path());
        }
        directory = directory
            .open_dir_nofollow(component)
            .map_err(|_| ResourceServiceError::unsafe_target())?;
    }
    Ok(directory)
}

fn join_relative(
    parent: &WorkspaceRelativePath,
    name: &str,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    WorkspaceRelativePath::parse(if parent.as_str().is_empty() {
        name.to_string()
    } else {
        format!("{}/{name}", parent.as_str())
    })
    .map_err(|_| ResourceServiceError::invalid_path())
}

fn parent_and_name(
    path: &WorkspaceRelativePath,
) -> Result<(WorkspaceRelativePath, String), ResourceServiceError> {
    let (parent, name) = path
        .as_str()
        .rsplit_once('/')
        .map_or(("", path.as_str()), |(parent, name)| (parent, name));
    if name.is_empty() {
        return Err(ResourceServiceError::invalid_path());
    }
    Ok((
        WorkspaceRelativePath::parse(parent).map_err(|_| ResourceServiceError::invalid_path())?,
        name.to_string(),
    ))
}

fn trusted_directory_metadata(directory: &Dir) -> Result<Metadata, ResourceServiceError> {
    let metadata = directory
        .dir_metadata()
        .map_err(|_| ResourceServiceError::unavailable())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        Ok(metadata)
    } else {
        Err(ResourceServiceError::unsafe_target())
    }
}

fn trusted_regular_file(metadata: &Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink() && link_count(metadata) == 1
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    MetadataExt::nlink(metadata)
}

#[cfg(windows)]
fn link_count(metadata: &Metadata) -> u64 {
    use cap_std::fs::MetadataExt as _;
    metadata.number_of_links().unwrap_or(0)
}

#[cfg(not(any(unix, windows)))]
fn link_count(_metadata: &Metadata) -> u64 {
    1
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    MetadataExt::dev(left) == MetadataExt::dev(right)
        && MetadataExt::ino(left) == MetadataExt::ino(right)
}

fn safe_size(value: u64) -> Result<SafeUnsignedInteger, ResourceServiceError> {
    SafeUnsignedInteger::new(value).map_err(|_| ResourceServiceError::unavailable())
}

fn modified_utc(metadata: &Metadata) -> Result<Rfc3339Utc, ResourceServiceError> {
    let value = metadata
        .modified()
        .map_err(|_| ResourceServiceError::unavailable())?
        .into_std();
    let value = OffsetDateTime::from(value)
        .format(&Rfc3339)
        .map_err(|_| ResourceServiceError::unavailable())?;
    Rfc3339Utc::parse(value).map_err(|_| ResourceServiceError::unavailable())
}

fn markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[derive(Clone, Copy)]
struct ResourceClassification {
    kind: ResourceKind,
    media_type: &'static str,
    previewable: bool,
}

fn classify_resource(name: &str, magic: &[u8; MAGIC_BYTES]) -> ResourceClassification {
    let lower = name.to_ascii_lowercase();
    let media_type = if lower.ends_with(".png") && magic.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if (lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        && magic.starts_with(b"\xff\xd8\xff")
    {
        Some("image/jpeg")
    } else if lower.ends_with(".gif")
        && (magic.starts_with(b"GIF87a") || magic.starts_with(b"GIF89a"))
    {
        Some("image/gif")
    } else if lower.ends_with(".webp") && magic.starts_with(b"RIFF") && &magic[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    };
    media_type.map_or(
        ResourceClassification {
            kind: ResourceKind::Attachment,
            media_type: "application/octet-stream",
            previewable: false,
        },
        |media_type| ResourceClassification {
            kind: ResourceKind::Image,
            media_type,
            previewable: true,
        },
    )
}

/// Retained resource capability for a later transport adapter.
///
/// A transport must call [`RetainedResource::verify_complete`] after it has
/// emitted the declared content length. Neither `Read::read` returning `Ok(0)`
/// nor an empty-buffer read proves stream integrity. Completion revalidates the
/// identity, metadata, workspace authority, and the SHA-256 of the exact bytes
/// emitted by this reader against the signed resource entry revision.
pub struct RetainedResource {
    snapshot: Arc<ActiveWorkspaceSnapshot>,
    parent: Dir,
    file: File,
    entry: ResourceEntryDto,
    expected: Metadata,
    remaining: u64,
    stream_digest: Sha256,
    verified_complete: bool,
}

impl RetainedResource {
    pub const fn entry(&self) -> &ResourceEntryDto {
        &self.entry
    }

    /// Verifies that the transport consumed exactly the declared content and
    /// that the emitted bytes still match the entry revision.
    ///
    /// Transport adapters must treat a failure as a failed response even when
    /// they have already read exactly `entry.size_bytes` bytes.
    pub fn verify_complete(&mut self) -> io::Result<()> {
        if self.verified_complete {
            return Ok(());
        }
        if self.remaining != 0 {
            return Err(stream_changed());
        }
        let mut sentinel = [0_u8; 1];
        if self.file.read(&mut sentinel)? != 0 {
            return Err(stream_changed());
        }
        let retained = self.file.metadata().map_err(|_| stream_changed())?;
        let named = self
            .parent
            .symlink_metadata(self.entry.name.as_str())
            .map_err(|_| stream_changed())?;
        let expected_modified = self.expected.modified().map_err(|_| stream_changed())?;
        if !trusted_regular_file(&retained)
            || !trusted_regular_file(&named)
            || !same_file(&self.expected, &retained)
            || !same_file(&self.expected, &named)
            || retained.len() != self.expected.len()
            || named.len() != self.expected.len()
            || retained.modified().ok() != Some(expected_modified)
            || named.modified().ok() != Some(expected_modified)
        {
            return Err(stream_changed());
        }
        let streamed_revision = format!("sha256:{:x}", self.stream_digest.clone().finalize());
        if streamed_revision != self.entry.revision.as_str() {
            return Err(stream_changed());
        }
        self.snapshot
            .authority()
            .verify_held_directory()
            .map_err(|_| stream_changed())?;
        self.verified_complete = true;
        Ok(())
    }
}

impl fmt::Debug for RetainedResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RetainedResource { capability: held, entry: opaque }")
    }
}

impl io::Read for RetainedResource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.verified_complete || self.remaining == 0 {
            return Ok(0);
        }
        let limit = usize::try_from(self.remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let read = self.file.read(&mut buffer[..limit])?;
        if read == 0 {
            return Err(stream_changed());
        }
        self.stream_digest.update(&buffer[..read]);
        self.remaining -= read as u64;
        Ok(read)
    }
}

fn stream_changed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "retained resource changed while streaming",
    )
}
