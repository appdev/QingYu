use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive};

use super::{
    parser::parse_theme_file,
    resources::{
        archive_entry_limit, normalize_relative_path, validate_path_aliases,
        validate_theme_directory, ValidatedThemeDirectory, MAX_PACKAGE_BYTES, MAX_PACKAGE_ENTRIES,
    },
    ParsedTheme, ThemeDescriptor, ThemeError, ThemeErrorCode,
};

const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_REGULAR_FILE: u32 = 0o100000;
const UNIX_DIRECTORY: u32 = 0o040000;
const INFO_ZIP_UNIX_EXTRA_FIELD: u16 = 0x000d;
const ASI_UNIX_EXTRA_FIELD: u16 = 0x756e;

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub(crate) enum PreparedThemeImport {
    LegacyCss(ParsedTheme),
    ResourcePackage(PreparedThemeDirectory),
}

impl PreparedThemeImport {
    pub(crate) fn descriptor(&self) -> &ThemeDescriptor {
        match self {
            Self::LegacyCss(theme) => &theme.descriptor,
            Self::ResourcePackage(package) => &package.validated().descriptor,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedThemeDirectory {
    validated: Option<ValidatedThemeDirectory>,
    staging: Option<StagingDirectory>,
}

impl PreparedThemeDirectory {
    pub(crate) fn validated(&self) -> &ValidatedThemeDirectory {
        self.validated
            .as_ref()
            .expect("prepared theme directory was already consumed")
    }

    pub(crate) fn publish_noreplace(
        mut self,
        target_name: &str,
    ) -> Result<ValidatedThemeDirectory, ThemeError> {
        let target = Path::new(target_name);
        if target.components().count() != 1 || target_name.is_empty() {
            return Err(unsafe_path(
                "Published theme package targets must name one catalog entry.",
            ));
        }
        let staging = self
            .staging
            .as_mut()
            .expect("prepared theme directory was already consumed");
        let validated = self
            .validated
            .as_ref()
            .expect("prepared theme directory was already consumed");
        revalidate_staging_directory(staging)?;
        validate_prepared_files(validated, staging)?;
        match staging.catalog.directory.symlink_metadata(target_name) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(ThemeError::new(
                    ThemeErrorCode::DuplicateTheme,
                    "The target theme package entry already exists.",
                ))
            }
            Err(error) => return Err(io_error(error)),
        }

        let source_root = staging.root.clone();
        let target_root = staging.catalog.root.join(target_name);
        revalidate_staging_directory(staging)?;
        rename_staging_noreplace(staging, target_name, &source_root, &target_root)?;
        staging.name = target_name.to_string();
        staging.root = target_root.clone();
        revalidate_staging_directory(staging)?;

        let refreshed = validate_theme_directory(&target_root, target_name)?;
        if refreshed.descriptor.id != validated.descriptor.id
            || refreshed.descriptor.fingerprint != validated.descriptor.fingerprint
            || refreshed.files != validated.files
        {
            return Err(unsafe_path(
                "Published theme package content differs from its prepared staging directory.",
            ));
        }
        validate_prepared_files(&refreshed, staging)?;
        self.validated = Some(refreshed);
        self.staging.take();
        Ok(self
            .validated
            .take()
            .expect("published theme directory validation is missing"))
    }
}

impl Drop for PreparedThemeDirectory {
    fn drop(&mut self) {
        if let Some(staging) = self.staging.take() {
            cleanup_staging_directory(staging);
        }
    }
}

fn regular_relative_file_snapshot(
    directory: &Dir,
    name: impl AsRef<Path>,
    limit: u64,
) -> Result<FileSnapshot, ThemeError> {
    let name = name.as_ref();
    let addressed = directory.symlink_metadata(name).map_err(io_error)?;
    if addressed.file_type().is_symlink() || !addressed.is_file() {
        return Err(unsafe_path(
            "Theme package files must be regular and cannot be links.",
        ));
    }
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut file = directory
        .open_with(name, &options)
        .map_err(|_| unsafe_path("A theme package file changed while being opened."))?;
    let retained = file.metadata().map_err(io_error)?;
    if !retained.is_file() || file_identity(&addressed) != file_identity(&retained) {
        return Err(unsafe_path(
            "A theme package file changed while being opened.",
        ));
    }
    let identity = file_identity(&retained);
    let expected_length = retained.len();
    if expected_length > limit {
        return Err(ThemeError::new(
            ThemeErrorCode::ThemeTooLarge,
            "Theme package files exceed the allowed size.",
        ));
    }

    let mut length = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        length = length.checked_add(read as u64).ok_or_else(|| {
            ThemeError::new(
                ThemeErrorCode::ThemeTooLarge,
                "Theme package files exceed the allowed size.",
            )
        })?;
        if length > limit {
            return Err(ThemeError::new(
                ThemeErrorCode::ThemeTooLarge,
                "Theme package files exceed the allowed size.",
            ));
        }
        hasher.update(&buffer[..read]);
    }

    let retained_after = file.metadata().map_err(io_error)?;
    let addressed_after = directory.symlink_metadata(name).map_err(io_error)?;
    if addressed_after.file_type().is_symlink()
        || !addressed_after.is_file()
        || file_identity(&retained_after) != identity
        || file_identity(&addressed_after) != identity
        || retained_after.len() != expected_length
        || addressed_after.len() != expected_length
        || length != expected_length
    {
        return Err(unsafe_path(
            "A theme package file changed while its content was verified.",
        ));
    }

    Ok(FileSnapshot {
        identity,
        length,
        digest: hasher.finalize().into(),
    })
}

fn rename_archive_noreplace(
    directory: &Dir,
    source_name: impl AsRef<Path>,
    target_name: impl AsRef<Path>,
    _source_ambient: &Path,
    _target_ambient: &Path,
) -> io::Result<()> {
    crate::atomic_noreplace::rename_noreplace(
        directory,
        source_name.as_ref(),
        directory,
        target_name.as_ref(),
    )
}

struct ArchiveEntry {
    index: usize,
    is_directory: bool,
    path: String,
    streamed_limit: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtractionHookPoint {
    AfterStagingCreate,
    BeforeFileCreate,
    AfterValidation,
}

pub(crate) fn prepare_external_theme(
    source: &Path,
    catalog_root: &Path,
) -> Result<PreparedThemeImport, ThemeError> {
    prepare_external_theme_with_hooks(source, catalog_root, &mut || {}, &mut |_, _| Ok(()))
}

fn prepare_external_theme_with_hooks(
    source: &Path,
    catalog_root: &Path,
    after_source_open: &mut dyn FnMut(),
    extraction_hook: &mut dyn FnMut(ExtractionHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<PreparedThemeImport, ThemeError> {
    let (mut source_file, source_metadata) = open_regular_source(source)?;
    after_source_open();
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or_else(|| unsafe_path("Theme imports must use the .css or .theme extension."))?;
    let file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| unsafe_path("Theme import file names must use UTF-8."))?;
    let bytes = read_bounded_source(&mut source_file, &source_metadata)?;

    match extension {
        "css" => parse_theme_file(&bytes, file_name).map(PreparedThemeImport::LegacyCss),
        "theme" => prepare_archive(bytes, file_name, catalog_root, extraction_hook),
        _ => Err(unsafe_path(
            "Theme imports must use the .css or .theme extension.",
        )),
    }
}

fn open_regular_source(
    source: &Path,
) -> Result<(cap_std::fs::File, cap_std::fs::Metadata), ThemeError> {
    let metadata = crate::storage_capability::ambient_symlink_metadata(source).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(unsafe_path(
            "Theme imports must be regular files and cannot be symbolic links.",
        ));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(package_too_large());
    }
    let parent = source
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = source
        .file_name()
        .ok_or_else(|| unsafe_path("Theme import paths must name a file."))?;
    let parent = Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map_err(io_error)?;
    let mut options = CapOpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let file = parent
        .open_with(name, &options)
        .map_err(|_| unsafe_path("The selected theme changed before it could be opened safely."))?;
    let retained = file.metadata().map_err(io_error)?;
    if !retained.is_file()
        || retained.dev() != metadata.dev()
        || retained.ino() != metadata.ino()
        || retained.len() != metadata.len()
    {
        return Err(unsafe_path(
            "The selected theme changed before it could be opened safely.",
        ));
    }
    Ok((file, metadata))
}

fn read_bounded_source(
    source: &mut cap_std::fs::File,
    initial: &cap_std::fs::Metadata,
) -> Result<Vec<u8>, ThemeError> {
    let mut bytes = Vec::with_capacity(initial.len() as usize);
    source
        .take(MAX_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        return Err(package_too_large());
    }
    let retained = source.metadata().map_err(io_error)?;
    if !retained.is_file()
        || retained.dev() != initial.dev()
        || retained.ino() != initial.ino()
        || retained.len() != initial.len()
        || retained.len() != bytes.len() as u64
    {
        return Err(unsafe_path(
            "The selected theme changed while it was being read.",
        ));
    }
    Ok(bytes)
}

fn prepare_archive(
    source_bytes: Vec<u8>,
    storage_name: &str,
    catalog_root: &Path,
    extraction_hook: &mut dyn FnMut(ExtractionHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<PreparedThemeImport, ThemeError> {
    let catalog = open_catalog_root(catalog_root)?;
    let raw_entries = validate_raw_zip_entries(&source_bytes)?;
    let source_bytes: Arc<[u8]> = source_bytes.into();
    let mut archive = ZipArchive::new(Cursor::new(source_bytes)).map_err(archive_error)?;
    let entries = inspect_archive(&mut archive, &raw_entries)?;
    let mut staging = create_staging_directory(catalog, extraction_hook)?;
    let result = (|| {
        extract_archive(&mut archive, &entries, &mut staging, extraction_hook)?;
        revalidate_staging_directory(&staging)?;
        let validated = validate_theme_directory(&staging.root, storage_name)?;
        extraction_hook(ExtractionHookPoint::AfterValidation, &staging.root)?;
        revalidate_staging_directory(&staging)?;
        validate_prepared_files(&validated, &staging)?;
        Ok(validated)
    })();
    match result {
        Ok(validated) => Ok(PreparedThemeImport::ResourcePackage(
            PreparedThemeDirectory {
                validated: Some(validated),
                staging: Some(staging),
            },
        )),
        Err(error) => {
            cleanup_staging_directory(staging);
            Err(error)
        }
    }
}

fn inspect_archive<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    raw_entries: &[RawZipEntry],
) -> Result<Vec<ArchiveEntry>, ThemeError> {
    if archive.len() > MAX_PACKAGE_ENTRIES || archive.len() != raw_entries.len() {
        return Err(package_too_large());
    }
    let mut declared_total = 0_u64;
    let mut normalized_paths = Vec::with_capacity(archive.len());
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(archive_error)?;
        if entry.encrypted() {
            return Err(invalid_archive(
                "Encrypted theme archives are not supported.",
            ));
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(invalid_archive(
                "Theme archives use an unsupported compression method.",
            ));
        }
        validate_archive_extra_fields(entry.extra_data())?;
        let raw_name = &raw_entries[index].name;
        let (path, is_directory) = normalize_archive_path(raw_name, entry.unix_mode())?;
        let streamed_limit = archive_entry_limit(&path, is_directory)?;
        if streamed_limit.is_some_and(|limit| entry.size() > limit) {
            return Err(package_too_large());
        }
        declared_total = declared_total
            .checked_add(entry.size())
            .ok_or_else(package_too_large)?;
        if declared_total > MAX_PACKAGE_BYTES {
            return Err(package_too_large());
        }
        normalized_paths.push(path.clone());
        entries.push(ArchiveEntry {
            index,
            is_directory,
            path,
            streamed_limit,
        });
    }
    validate_path_aliases(&normalized_paths)?;
    validate_prefix_collisions(&entries)?;
    Ok(entries)
}

#[derive(Debug)]
struct RawZipEntry {
    name: String,
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
}

fn validate_raw_zip_entries(bytes: &[u8]) -> Result<Vec<RawZipEntry>, ThemeError> {
    const END_SIZE: usize = 22;
    const CENTRAL_SIZE: usize = 46;
    let search_start = bytes.len().saturating_sub(END_SIZE + u16::MAX as usize);
    let end_offset = (search_start..=bytes.len().saturating_sub(END_SIZE))
        .rev()
        .find(|offset| {
            bytes.get(*offset..*offset + 4) == Some(b"PK\x05\x06".as_slice())
                && read_u16(bytes, *offset + 20)
                    .is_some_and(|comment| *offset + END_SIZE + comment as usize == bytes.len())
        })
        .ok_or_else(|| invalid_archive("Theme archive end record is missing or malformed."))?;
    let disk = read_u16(bytes, end_offset + 4).ok_or_else(malformed_archive)?;
    let central_disk = read_u16(bytes, end_offset + 6).ok_or_else(malformed_archive)?;
    let disk_entries = read_u16(bytes, end_offset + 8).ok_or_else(malformed_archive)?;
    let entry_count = read_u16(bytes, end_offset + 10).ok_or_else(malformed_archive)?;
    let central_size = read_u32(bytes, end_offset + 12).ok_or_else(malformed_archive)?;
    let central_offset = read_u32(bytes, end_offset + 16).ok_or_else(malformed_archive)?;
    if disk != 0
        || central_disk != 0
        || disk_entries != entry_count
        || entry_count == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
    {
        return Err(invalid_archive(
            "Theme archives cannot use multi-disk or unbounded ZIP directory records.",
        ));
    }
    if entry_count as usize > MAX_PACKAGE_ENTRIES {
        return Err(package_too_large());
    }
    let central_offset = central_offset as usize;
    let central_end = central_offset
        .checked_add(central_size as usize)
        .ok_or_else(malformed_archive)?;
    if central_end != end_offset || central_end > bytes.len() {
        return Err(invalid_archive(
            "Theme archive central directory bounds are invalid.",
        ));
    }

    let mut offset = central_offset;
    let mut entries = Vec::with_capacity(entry_count as usize);
    for _ in 0..entry_count {
        if offset
            .checked_add(CENTRAL_SIZE)
            .is_none_or(|end| end > central_end)
            || bytes.get(offset..offset + 4) != Some(b"PK\x01\x02".as_slice())
        {
            return Err(malformed_archive());
        }
        let name_length = read_u16(bytes, offset + 28).ok_or_else(malformed_archive)? as usize;
        let extra_length = read_u16(bytes, offset + 30).ok_or_else(malformed_archive)? as usize;
        let comment_length = read_u16(bytes, offset + 32).ok_or_else(malformed_archive)? as usize;
        let disk_start = read_u16(bytes, offset + 34).ok_or_else(malformed_archive)?;
        let flags = read_u16(bytes, offset + 8).ok_or_else(malformed_archive)?;
        let method = read_u16(bytes, offset + 10).ok_or_else(malformed_archive)?;
        let crc32 = read_u32(bytes, offset + 16).ok_or_else(malformed_archive)?;
        let compressed_size = read_u32(bytes, offset + 20).ok_or_else(malformed_archive)?;
        let uncompressed_size = read_u32(bytes, offset + 24).ok_or_else(malformed_archive)?;
        let local_offset = read_u32(bytes, offset + 42).ok_or_else(malformed_archive)?;
        if disk_start != 0
            || compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
            || local_offset == u32::MAX
        {
            return Err(invalid_archive(
                "Theme archive entries cannot span disks or use ZIP64 sentinels.",
            ));
        }
        let name_start = offset + CENTRAL_SIZE;
        let name_end = name_start
            .checked_add(name_length)
            .ok_or_else(malformed_archive)?;
        let extra_end = name_end
            .checked_add(extra_length)
            .ok_or_else(malformed_archive)?;
        let next = extra_end
            .checked_add(comment_length)
            .ok_or_else(malformed_archive)?;
        if next > central_end {
            return Err(malformed_archive());
        }
        let central_name = bytes
            .get(name_start..name_end)
            .ok_or_else(malformed_archive)?;
        let central_name = std::str::from_utf8(central_name)
            .map_err(|_| unsafe_path("Theme archive paths must use UTF-8."))?;
        validate_archive_extra_fields(Some(
            bytes
                .get(name_end..extra_end)
                .ok_or_else(malformed_archive)?,
        ))?;
        let entry = RawZipEntry {
            name: central_name.to_string(),
            flags,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
        };
        validate_matching_local_header(bytes, local_offset as usize, &entry)?;
        entries.push(entry);
        offset = next;
    }
    if offset != central_end {
        return Err(malformed_archive());
    }
    Ok(entries)
}

fn validate_matching_local_header(
    bytes: &[u8],
    offset: usize,
    central: &RawZipEntry,
) -> Result<(), ThemeError> {
    const LOCAL_SIZE: usize = 30;
    if offset
        .checked_add(LOCAL_SIZE)
        .is_none_or(|end| end > bytes.len())
        || bytes.get(offset..offset + 4) != Some(b"PK\x03\x04".as_slice())
    {
        return Err(malformed_archive());
    }
    let name_length = read_u16(bytes, offset + 26).ok_or_else(malformed_archive)? as usize;
    let extra_length = read_u16(bytes, offset + 28).ok_or_else(malformed_archive)? as usize;
    let flags = read_u16(bytes, offset + 6).ok_or_else(malformed_archive)?;
    let method = read_u16(bytes, offset + 8).ok_or_else(malformed_archive)?;
    let crc32 = read_u32(bytes, offset + 14).ok_or_else(malformed_archive)?;
    let compressed_size = read_u32(bytes, offset + 18).ok_or_else(malformed_archive)?;
    let uncompressed_size = read_u32(bytes, offset + 22).ok_or_else(malformed_archive)?;
    if compressed_size == u32::MAX || uncompressed_size == u32::MAX {
        return Err(invalid_archive(
            "Theme archive local headers cannot use ZIP64 sentinels.",
        ));
    }
    let name_start = offset + LOCAL_SIZE;
    let name_end = name_start
        .checked_add(name_length)
        .ok_or_else(malformed_archive)?;
    let local_name = bytes
        .get(name_start..name_end)
        .ok_or_else(malformed_archive)?;
    std::str::from_utf8(local_name)
        .map_err(|_| unsafe_path("Theme archive paths must use UTF-8."))?;
    if local_name != central.name.as_bytes() {
        return Err(unsafe_path(
            "Theme archive local and central paths must match exactly.",
        ));
    }
    let extra_end = name_end
        .checked_add(extra_length)
        .ok_or_else(malformed_archive)?;
    validate_archive_extra_fields(Some(
        bytes
            .get(name_end..extra_end)
            .ok_or_else(malformed_archive)?,
    ))?;
    if flags != central.flags || method != central.method {
        return Err(invalid_archive(
            "Theme archive local flags or compression method disagree with the central directory.",
        ));
    }
    let uses_descriptor = flags & (1 << 3) != 0;
    if uses_descriptor {
        if crc32 != 0 || compressed_size != 0 || uncompressed_size != 0 {
            return Err(invalid_archive(
                "Theme archive data-descriptor local fields must be zero.",
            ));
        }
        validate_data_descriptor(bytes, extra_end, central)?;
    } else if crc32 != central.crc32
        || compressed_size != central.compressed_size
        || uncompressed_size != central.uncompressed_size
    {
        return Err(invalid_archive(
            "Theme archive local checksums or sizes disagree with the central directory.",
        ));
    }
    Ok(())
}

fn validate_data_descriptor(
    bytes: &[u8],
    data_start: usize,
    central: &RawZipEntry,
) -> Result<(), ThemeError> {
    let mut descriptor = data_start
        .checked_add(central.compressed_size as usize)
        .ok_or_else(malformed_archive)?;
    if read_u32(bytes, descriptor) == Some(0x0807_4b50) {
        descriptor = descriptor.checked_add(4).ok_or_else(malformed_archive)?;
    }
    let crc32 = read_u32(bytes, descriptor).ok_or_else(malformed_archive)?;
    let compressed_size = read_u32(bytes, descriptor + 4).ok_or_else(malformed_archive)?;
    let uncompressed_size = read_u32(bytes, descriptor + 8).ok_or_else(malformed_archive)?;
    if crc32 != central.crc32
        || compressed_size != central.compressed_size
        || uncompressed_size != central.uncompressed_size
    {
        return Err(invalid_archive(
            "Theme archive data descriptor disagrees with the central directory.",
        ));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn malformed_archive() -> ThemeError {
    invalid_archive("Theme archive directory records are malformed.")
}

fn normalize_archive_path(
    raw_name: &str,
    unix_mode: Option<u32>,
) -> Result<(String, bool), ThemeError> {
    if raw_name.is_empty()
        || raw_name.starts_with(['/', '\\'])
        || raw_name.contains(['\0', '\\', ':'])
        || raw_name.chars().any(char::is_control)
    {
        return Err(unsafe_path(
            "Theme archive paths must be safe relative paths.",
        ));
    }
    let slash_directory = raw_name.ends_with('/');
    let body = raw_name.strip_suffix('/').unwrap_or(raw_name);
    if body.is_empty() || body.split('/').any(is_unsafe_portable_segment) {
        return Err(unsafe_path(
            "Theme archive paths contain an unsafe segment.",
        ));
    }
    let mode_type = unix_mode.unwrap_or(0) & UNIX_FILE_TYPE_MASK;
    let is_directory = match mode_type {
        0 => slash_directory,
        UNIX_REGULAR_FILE if !slash_directory => false,
        UNIX_DIRECTORY => true,
        _ => {
            return Err(unsafe_path(
                "Theme archives cannot contain links or special files.",
            ))
        }
    };
    let normalized = normalize_relative_path(Path::new(body))?;
    Ok((normalized, is_directory))
}

fn is_unsafe_portable_segment(segment: &str) -> bool {
    if segment.is_empty()
        || matches!(segment, "." | "..")
        || segment.ends_with(['.', ' '])
        || segment.chars().any(char::is_control)
        || segment.contains(':')
    {
        return true;
    }
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end_matches(['.', ' ']);
    let uppercase = stem.to_ascii_uppercase();
    matches!(
        uppercase.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || uppercase.strip_prefix("COM").is_some_and(|number| {
        matches!(
            number,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    }) || uppercase.strip_prefix("LPT").is_some_and(|number| {
        matches!(
            number,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

fn validate_archive_extra_fields(extra: Option<&[u8]>) -> Result<(), ThemeError> {
    let Some(mut extra) = extra else {
        return Ok(());
    };
    while !extra.is_empty() {
        if extra.len() < 4 {
            return Err(invalid_archive("Theme archive extra fields are malformed."));
        }
        let id = u16::from_le_bytes([extra[0], extra[1]]);
        let length = u16::from_le_bytes([extra[2], extra[3]]) as usize;
        if extra.len() < 4 + length {
            return Err(invalid_archive("Theme archive extra fields are malformed."));
        }
        if id == 0x0001 {
            return Err(invalid_archive(
                "Theme archive entries cannot use ZIP64 extra fields.",
            ));
        }
        if matches!(id, INFO_ZIP_UNIX_EXTRA_FIELD | ASI_UNIX_EXTRA_FIELD) {
            return Err(unsafe_path(
                "Theme archives cannot contain hard-link metadata.",
            ));
        }
        extra = &extra[4 + length..];
    }
    Ok(())
}

fn validate_prefix_collisions(entries: &[ArchiveEntry]) -> Result<(), ThemeError> {
    let directories = entries
        .iter()
        .filter(|entry| entry.is_directory)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let files = entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    for path in files {
        if directories.contains(path)
            || entries
                .iter()
                .any(|entry| entry.path.starts_with(&format!("{path}/")))
        {
            return Err(unsafe_path(
                "Theme archive paths contain file and directory aliases.",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
    identity: FileIdentity,
    length: u64,
    digest: [u8; 32],
}

fn file_identity<T: MetadataExt>(metadata: &T) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

#[derive(Debug)]
struct CatalogDirectory {
    directory: Dir,
    root: PathBuf,
    identity: FileIdentity,
}

#[derive(Debug)]
struct StagingDirectory {
    catalog: CatalogDirectory,
    directory: Dir,
    name: String,
    root: PathBuf,
    identity: FileIdentity,
    directories: BTreeMap<String, FileIdentity>,
    files: BTreeMap<String, FileSnapshot>,
}

struct PendingStagingGuard<'a> {
    catalog: &'a Dir,
    name: &'a str,
    armed: bool,
}

impl Drop for PendingStagingGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _cleanup = self.catalog.remove_dir_all(self.name);
        }
    }
}

fn open_catalog_root(catalog_root: &Path) -> Result<CatalogDirectory, ThemeError> {
    let addressed =
        crate::storage_capability::ambient_symlink_metadata(catalog_root).map_err(io_error)?;
    if addressed.file_type().is_symlink() || !addressed.is_dir() {
        return Err(unsafe_path(
            "The theme catalog root must be a regular directory.",
        ));
    }
    let directory = match (catalog_root.parent(), catalog_root.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            Dir::open_ambient_dir(parent, cap_std::ambient_authority())
                .and_then(|parent| parent.open_dir_nofollow(name))
                .map_err(|_| unsafe_path("The theme catalog root changed while being opened."))?
        }
        _ => Dir::open_ambient_dir(catalog_root, cap_std::ambient_authority()).map_err(io_error)?,
    };
    let retained = directory.dir_metadata().map_err(io_error)?;
    if !retained.is_dir() || file_identity(&addressed) != file_identity(&retained) {
        return Err(unsafe_path(
            "The theme catalog root changed while being opened.",
        ));
    }
    Ok(CatalogDirectory {
        directory,
        root: catalog_root.to_path_buf(),
        identity: file_identity(&retained),
    })
}

fn create_staging_directory(
    catalog: CatalogDirectory,
    hook: &mut dyn FnMut(ExtractionHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<StagingDirectory, ThemeError> {
    for _attempt in 0..1024 {
        let counter = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!(".qingyu-theme-{}-{counter}.dir", std::process::id());
        match catalog.directory.create_dir(&name) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error(error)),
        }
        let root = catalog.root.join(&name);
        let (directory, identity) = {
            let mut guard = PendingStagingGuard {
                catalog: &catalog.directory,
                name: &name,
                armed: true,
            };
            hook(ExtractionHookPoint::AfterStagingCreate, &root)?;
            let addressed = catalog
                .directory
                .symlink_metadata(&name)
                .map_err(io_error)?;
            if addressed.file_type().is_symlink() || !addressed.is_dir() {
                return Err(unsafe_path(
                    "The theme staging directory changed during creation.",
                ));
            }
            let directory = catalog
                .directory
                .open_dir_nofollow(&name)
                .map_err(|_| unsafe_path("The theme staging directory changed during creation."))?;
            let retained = directory.dir_metadata().map_err(io_error)?;
            if !retained.is_dir() || file_identity(&addressed) != file_identity(&retained) {
                return Err(unsafe_path(
                    "The theme staging directory changed during creation.",
                ));
            }
            guard.armed = false;
            (directory, file_identity(&retained))
        };
        return Ok(StagingDirectory {
            catalog,
            directory,
            name,
            root,
            identity,
            directories: BTreeMap::new(),
            files: BTreeMap::new(),
        });
    }
    Err(ThemeError::new(
        ThemeErrorCode::Io,
        "Could not reserve a unique theme staging directory.",
    ))
}

fn extract_archive<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    entries: &[ArchiveEntry],
    staging: &mut StagingDirectory,
    hook: &mut dyn FnMut(ExtractionHookPoint, &Path) -> Result<(), ThemeError>,
) -> Result<(), ThemeError> {
    let mut actual_total = 0_u64;
    for specification in entries {
        if specification.is_directory {
            open_or_create_directory(staging, Path::new(&specification.path))?;
            continue;
        }
        let relative = Path::new(&specification.path);
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let directory = open_or_create_directory(staging, parent)?;
        let file_name = relative
            .file_name()
            .ok_or_else(|| unsafe_path("Theme archive file paths must name a file."))?;
        let target = staging.root.join(relative);
        hook(ExtractionHookPoint::BeforeFileCreate, &target)?;
        let mut entry = archive
            .by_index(specification.index)
            .map_err(archive_error)?;
        let mut options = CapOpenOptions::new();
        options
            .create_new(true)
            .write(true)
            .follow(FollowSymlinks::No);
        let mut output = directory
            .open_with(file_name, &options)
            .map_err(|_| unsafe_path("Theme archive extraction encountered an occupied path."))?;
        let created = output.metadata().map_err(io_error)?;
        if !created.is_file() {
            return Err(unsafe_path(
                "Theme archive extraction created a non-regular file.",
            ));
        }
        let expected_identity = file_identity(&created);
        let mut hasher = Sha256::new();
        let mut file_total = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(|error| {
                invalid_archive(format!("Theme archive data is invalid: {error}"))
            })?;
            if read == 0 {
                break;
            }
            file_total = file_total
                .checked_add(read as u64)
                .ok_or_else(package_too_large)?;
            actual_total = actual_total
                .checked_add(read as u64)
                .ok_or_else(package_too_large)?;
            if specification
                .streamed_limit
                .is_some_and(|limit| file_total > limit)
                || actual_total > MAX_PACKAGE_BYTES
            {
                return Err(package_too_large());
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(io_error)?;
        }
        output.sync_all().map_err(io_error)?;
        let retained = output.metadata().map_err(io_error)?;
        let addressed = directory.symlink_metadata(file_name).map_err(io_error)?;
        if addressed.file_type().is_symlink()
            || !addressed.is_file()
            || file_identity(&retained) != expected_identity
            || file_identity(&addressed) != expected_identity
            || retained.len() != file_total
            || addressed.len() != file_total
        {
            return Err(unsafe_path(
                "Theme archive extraction output changed while being written.",
            ));
        }
        let expected_snapshot = FileSnapshot {
            identity: expected_identity,
            length: file_total,
            digest: hasher.finalize().into(),
        };
        let current_snapshot = regular_relative_file_snapshot(
            &directory,
            file_name,
            specification.streamed_limit.unwrap_or(MAX_PACKAGE_BYTES),
        )?;
        if current_snapshot != expected_snapshot {
            return Err(unsafe_path(
                "Theme archive extraction output changed after it was written.",
            ));
        }
        staging
            .files
            .insert(specification.path.clone(), expected_snapshot);
    }
    Ok(())
}

fn open_or_create_directory(
    staging: &mut StagingDirectory,
    relative: &Path,
) -> Result<Dir, ThemeError> {
    let mut current = staging.directory.try_clone().map_err(io_error)?;
    let mut current_path = PathBuf::new();
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(unsafe_path("Theme archive directory paths are unsafe."));
        };
        current_path.push(segment);
        let key = current_path.to_string_lossy().replace('\\', "/");
        match current.create_dir(segment) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !staging.directories.contains_key(&key) {
                    return Err(unsafe_path(
                        "Theme archive extraction encountered an occupied path.",
                    ));
                }
            }
            Err(error) => return Err(io_error(error)),
        }
        let addressed = current.symlink_metadata(segment).map_err(io_error)?;
        if addressed.file_type().is_symlink() || !addressed.is_dir() {
            return Err(unsafe_path(
                "Theme archive extraction encountered an unsafe directory.",
            ));
        }
        let child = current
            .open_dir_nofollow(segment)
            .map_err(|_| unsafe_path("Theme archive directory changed while being opened."))?;
        let retained = child.dir_metadata().map_err(io_error)?;
        let identity = file_identity(&retained);
        if !retained.is_dir()
            || file_identity(&addressed) != identity
            || staging
                .directories
                .get(&key)
                .is_some_and(|expected| *expected != identity)
        {
            return Err(unsafe_path(
                "Theme archive directory changed while being opened.",
            ));
        }
        staging.directories.insert(key, identity);
        current = child;
    }
    Ok(current)
}

fn open_existing_directory(staging: &StagingDirectory, relative: &Path) -> Result<Dir, ThemeError> {
    let mut current = staging.directory.try_clone().map_err(io_error)?;
    let mut current_path = PathBuf::new();
    for component in relative.components() {
        let std::path::Component::Normal(segment) = component else {
            return Err(unsafe_path("Theme archive directory paths are unsafe."));
        };
        current_path.push(segment);
        let key = current_path.to_string_lossy().replace('\\', "/");
        let expected = staging
            .directories
            .get(&key)
            .ok_or_else(|| unsafe_path("Theme archive directory identity is missing."))?;
        let addressed = current.symlink_metadata(segment).map_err(io_error)?;
        if addressed.file_type().is_symlink()
            || !addressed.is_dir()
            || file_identity(&addressed) != *expected
        {
            return Err(unsafe_path(
                "Theme archive directory changed after extraction.",
            ));
        }
        let child = current
            .open_dir_nofollow(segment)
            .map_err(|_| unsafe_path("Theme archive directory changed after extraction."))?;
        let retained = child.dir_metadata().map_err(io_error)?;
        if !retained.is_dir() || file_identity(&retained) != *expected {
            return Err(unsafe_path(
                "Theme archive directory changed after extraction.",
            ));
        }
        current = child;
    }
    Ok(current)
}

fn revalidate_staging_directory(staging: &StagingDirectory) -> Result<(), ThemeError> {
    let current_catalog = open_catalog_root(&staging.catalog.root)?;
    if current_catalog.identity != staging.catalog.identity
        || file_identity(&staging.catalog.directory.dir_metadata().map_err(io_error)?)
            != staging.catalog.identity
    {
        return Err(unsafe_path(
            "The theme catalog root changed during extraction.",
        ));
    }
    let addressed = staging
        .catalog
        .directory
        .symlink_metadata(&staging.name)
        .map_err(io_error)?;
    if addressed.file_type().is_symlink()
        || !addressed.is_dir()
        || file_identity(&addressed) != staging.identity
    {
        return Err(unsafe_path(
            "The theme staging directory changed during extraction.",
        ));
    }
    let reopened = staging
        .catalog
        .directory
        .open_dir_nofollow(&staging.name)
        .map_err(|_| unsafe_path("The theme staging directory changed during extraction."))?;
    if file_identity(&reopened.dir_metadata().map_err(io_error)?) != staging.identity
        || file_identity(&staging.directory.dir_metadata().map_err(io_error)?) != staging.identity
    {
        return Err(unsafe_path(
            "The theme staging directory changed during extraction.",
        ));
    }
    for path in staging.directories.keys() {
        open_existing_directory(staging, Path::new(path))?;
    }
    for (path, expected) in &staging.files {
        let relative = Path::new(path);
        let parent =
            open_existing_directory(staging, relative.parent().unwrap_or_else(|| Path::new("")))?;
        let name = relative
            .file_name()
            .ok_or_else(|| unsafe_path("Theme archive file identity is missing."))?;
        let addressed = parent.symlink_metadata(name).map_err(io_error)?;
        if addressed.file_type().is_symlink()
            || !addressed.is_file()
            || file_identity(&addressed) != expected.identity
        {
            return Err(unsafe_path("Theme archive file changed after extraction."));
        }
        let limit = archive_entry_limit(path, false)?
            .ok_or_else(|| unsafe_path("Theme archive file limit is missing."))?;
        if regular_relative_file_snapshot(&parent, name, limit)? != *expected {
            return Err(unsafe_path("Theme archive file changed after extraction."));
        }
    }
    Ok(())
}

fn validate_prepared_files(
    validated: &ValidatedThemeDirectory,
    staging: &StagingDirectory,
) -> Result<(), ThemeError> {
    if validated.files.len() != staging.files.len() {
        return Err(unsafe_path(
            "Validated theme files disagree with the prepared staging directory.",
        ));
    }
    for file in &validated.files {
        let expected = staging.files.get(&file.relative_path).ok_or_else(|| {
            unsafe_path("Validated theme files disagree with the prepared staging directory.")
        })?;
        let mut hasher = Sha256::new();
        hasher.update(&file.bytes);
        let digest: [u8; 32] = hasher.finalize().into();
        if file.bytes.len() as u64 != expected.length || digest != expected.digest {
            return Err(unsafe_path(
                "Validated theme file content changed before preparation completed.",
            ));
        }
    }
    Ok(())
}

fn rename_staging_noreplace(
    staging: &StagingDirectory,
    target_name: &str,
    source_ambient: &Path,
    target_ambient: &Path,
) -> Result<(), ThemeError> {
    revalidate_staging_directory(staging)?;
    rename_archive_noreplace(
        &staging.catalog.directory,
        &staging.name,
        target_name,
        source_ambient,
        target_ambient,
    )
    .map_err(io_error)?;
    let retained_catalog = staging.catalog.directory.dir_metadata().map_err(io_error)?;
    if file_identity(&retained_catalog) != staging.catalog.identity {
        return Err(unsafe_path(
            "The theme catalog root changed during package publication.",
        ));
    }
    Ok(())
}

fn cleanup_staging_directory(staging: StagingDirectory) {
    if let Ok(addressed) = staging.catalog.directory.symlink_metadata(&staging.name) {
        if file_identity(&addressed) != staging.identity {
            if addressed.file_type().is_symlink() || addressed.is_file() {
                let _cleanup = staging.catalog.directory.remove_file(&staging.name);
            } else if addressed.is_dir() {
                let _cleanup = staging.catalog.directory.remove_dir_all(&staging.name);
            }
        }
    }
    let _cleanup = staging.directory.remove_open_dir_all();
}

fn archive_error(error: zip::result::ZipError) -> ThemeError {
    invalid_archive(format!("Theme archive is invalid: {error}"))
}

fn io_error(error: std::io::Error) -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::Io,
        format!("Theme archive operation failed: {error}"),
    )
}

fn invalid_archive(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorCode::InvalidArchive, message)
}

fn unsafe_path(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorCode::UnsafePath, message)
}

fn package_too_large() -> ThemeError {
    ThemeError::new(
        ThemeErrorCode::ThemeTooLarge,
        "Theme archives exceed a source, entry, path, file, or 32 MiB output limit.",
    )
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::Path};

    use tempfile::tempdir;
    use zip::{
        write::{FullFileOptions, SimpleFileOptions},
        CompressionMethod, ZipWriter,
    };

    use super::{
        prepare_external_theme, prepare_external_theme_with_hooks, ExtractionHookPoint,
        PreparedThemeImport, MAX_ARCHIVE_BYTES,
    };
    use crate::themes::ThemeErrorCode;

    const MANIFEST: &[u8] = br##"{"schemaVersion":1,"id":"fixture-theme","name":"Fixture Theme","appearance":"light","entry":"theme.css","author":"QingYu","version":"1.0.0","preview":{"background":"#ffffff","panel":"#f6f8fa","text":"#333333","accent":"#e95f59"}}"##;
    const CSS: &[u8] = b":root { --theme-accent: #e95f59; }";

    fn zip_entries(method: CompressionMethod, entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in entries {
            archive.start_file(*name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    fn minimal_package(method: CompressionMethod) -> Vec<u8> {
        zip_entries(method, &[("manifest.json", MANIFEST), ("theme.css", CSS)])
    }

    fn streaming_package(method: CompressionMethod) -> Vec<u8> {
        let mut archive = ZipWriter::new_stream(Vec::new());
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, bytes) in [("manifest.json", MANIFEST), ("theme.css", CSS)] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    fn package_with_asset(method: CompressionMethod) -> Vec<u8> {
        zip_entries(
            method,
            &[
                ("manifest.json", MANIFEST),
                (
                    "theme.css",
                    b":root { background: url('./assets/image.png'); }",
                ),
                ("assets/image.png", b"\x89PNG\r\n\x1a\nresource-bytes"),
                ("licenses/NOTICE.txt", b"Notice bytes\n"),
            ],
        )
    }

    fn write_source(root: &Path, bytes: &[u8]) -> std::path::PathBuf {
        let source = root.join("fixture.theme");
        fs::write(&source, bytes).unwrap();
        source
    }

    fn reject_archive(bytes: &[u8]) -> ThemeErrorCode {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let source = write_source(temp.path(), bytes);
        let error = prepare_external_theme(&source, &catalog).unwrap_err();
        assert_no_staging(&catalog);
        error.code
    }

    fn assert_no_staging(catalog: &Path) {
        let entries = fs::read_dir(catalog)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            entries.is_empty(),
            "archive failure left catalog entries: {:?}",
            entries
                .iter()
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>()
        );
    }

    fn patch_name(bytes: &mut [u8], expected: &[u8], replacement: &[u8]) {
        assert_eq!(expected.len(), replacement.len());
        let mut replacements = 0;
        for offset in 0..=bytes.len() - expected.len() {
            if &bytes[offset..offset + expected.len()] == expected {
                bytes[offset..offset + expected.len()].copy_from_slice(replacement);
                replacements += 1;
            }
        }
        assert_eq!(replacements, 2, "expected local and central names");
    }

    fn patch_headers(bytes: &mut [u8], mut patch: impl FnMut(&mut [u8], bool)) {
        let mut offset = 0;
        let mut headers = 0;
        while offset + 4 <= bytes.len() {
            let signature = &bytes[offset..offset + 4];
            if signature == b"PK\x03\x04" {
                patch(&mut bytes[offset..], false);
                headers += 1;
                offset += 30;
            } else if signature == b"PK\x01\x02" {
                patch(&mut bytes[offset..], true);
                headers += 1;
                offset += 46;
            } else {
                offset += 1;
            }
        }
        assert!(headers >= 2);
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    #[test]
    fn prepares_valid_stored_and_deflated_packages() {
        for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
            let temp = tempdir().unwrap();
            let catalog = temp.path().join("catalog");
            fs::create_dir(&catalog).unwrap();
            let source = temp.path().join("fixture.theme");
            fs::write(&source, minimal_package(method)).unwrap();

            let prepared = prepare_external_theme(&source, &catalog).unwrap();

            assert_eq!(prepared.descriptor().id, "fixture-theme");
        }
    }

    #[test]
    fn rejects_non_utf8_raw_entry_names() {
        let mut bytes = zip_entries(
            CompressionMethod::Stored,
            &[("assets/x.png", b"\x89PNG\r\n\x1a\nrest")],
        );
        patch_name(&mut bytes, b"assets/x.png", b"assets/\xff.png");

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafePath);
    }

    #[test]
    fn rejects_non_utf8_central_names_even_with_a_valid_unicode_path_extra_field() {
        let invalid_raw_name = b"assets/\xff.png";
        let unicode_name = b"assets/y.png";
        let mut unicode_extra = vec![1];
        unicode_extra.extend_from_slice(&crc32(invalid_raw_name).to_le_bytes());
        unicode_extra.extend_from_slice(unicode_name);
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let mut options = FullFileOptions::default();
        options
            .add_extra_data(0xcafe, unicode_extra.into_boxed_slice(), true)
            .unwrap();
        archive.start_file("assets/x.png", options).unwrap();
        archive.write_all(b"\x89PNG\r\n\x1a\nrest").unwrap();
        let mut bytes = archive.finish().unwrap().into_inner();
        patch_name(&mut bytes, b"assets/x.png", invalid_raw_name);
        let marker = [0xfe, 0xca, 17, 0];
        let marker_offset = bytes
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("Unicode Path extra-field marker");
        bytes[marker_offset..marker_offset + 2].copy_from_slice(&0x7075_u16.to_le_bytes());

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafePath);
    }

    #[test]
    fn rejects_sources_larger_than_sixteen_mib_before_zip_parsing() {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let source = temp.path().join("oversized.theme");
        let file = fs::File::create(&source).unwrap();
        file.set_len(MAX_ARCHIVE_BYTES + 1).unwrap();

        let error = prepare_external_theme(&source, &catalog).unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::ThemeTooLarge);
        assert_no_staging(&catalog);
    }

    #[test]
    fn rejects_css_and_archive_sources_that_grow_after_open() {
        for (name, initial) in [
            ("growing.css", CSS.to_vec()),
            ("growing.theme", minimal_package(CompressionMethod::Stored)),
        ] {
            let temp = tempdir().unwrap();
            let catalog = temp.path().join("catalog");
            fs::create_dir(&catalog).unwrap();
            let source = temp.path().join(name);
            fs::write(&source, &initial).unwrap();
            let source_for_hook = source.clone();

            let error = prepare_external_theme_with_hooks(
                &source,
                &catalog,
                &mut || {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&source_for_hook)
                        .unwrap()
                        .set_len(MAX_ARCHIVE_BYTES + 1)
                        .unwrap();
                },
                &mut |_, _| Ok(()),
            )
            .unwrap_err();

            assert_eq!(error.code, ThemeErrorCode::ThemeTooLarge, "{name}");
            assert_no_staging(&catalog);
        }
    }

    #[test]
    fn rejects_declared_uncompressed_output_above_thirty_two_mib() {
        let oversized = vec![0_u8; 32 * 1024 * 1024 + 1];
        let bytes = zip_entries(
            CompressionMethod::Deflated,
            &[("licenses/large.txt", &oversized)],
        );

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::ThemeTooLarge);
    }

    #[test]
    fn rejects_streamed_output_above_thirty_two_mib_even_when_declared_size_is_small() {
        let oversized = vec![0_u8; 32 * 1024 * 1024 + 1];
        let mut bytes = zip_entries(
            CompressionMethod::Deflated,
            &[("licenses/large.txt", &oversized)],
        );
        patch_headers(&mut bytes, |header, central| {
            let uncompressed_size_offset = if central { 24 } else { 22 };
            header[uncompressed_size_offset..uncompressed_size_offset + 4]
                .copy_from_slice(&1_u32.to_le_bytes());
        });

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::ThemeTooLarge);
    }

    #[test]
    fn rejects_more_than_two_hundred_fifty_six_entries() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        for index in 0..257 {
            archive
                .add_directory(format!("assets/d{index}"), SimpleFileOptions::default())
                .unwrap();
        }
        let bytes = archive.finish().unwrap().into_inner();

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::ThemeTooLarge);
    }

    #[test]
    fn rejects_unsafe_overlong_and_overdeep_entry_paths() {
        let overlong = format!("assets/{}.png", "a".repeat(241));
        let overdeep = format!("assets/{}/image.png", vec!["d"; 15].join("/"));
        let attacks = vec![
            "/absolute.png".to_string(),
            "C:/drive.png".to_string(),
            "\\\\server\\share.png".to_string(),
            "nul\0tail.png".to_string(),
            "assets//empty.png".to_string(),
            "./theme.css".to_string(),
            "../outside.css".to_string(),
            "assets\\backslash.png".to_string(),
            overlong,
            overdeep,
        ];
        for attack in attacks {
            let bytes = zip_entries(CompressionMethod::Stored, &[(attack.as_str(), b"x")]);
            assert!(
                matches!(
                    reject_archive(&bytes),
                    ThemeErrorCode::UnsafePath | ThemeErrorCode::ThemeTooLarge
                ),
                "path {attack:?}"
            );
        }
    }

    #[test]
    fn rejects_percent_encoded_traversal_when_css_references_it() {
        let css = b":root { background: url('./assets/%2e%2e/evil.png'); }";
        let bytes = zip_entries(
            CompressionMethod::Stored,
            &[
                ("manifest.json", MANIFEST),
                ("theme.css", css),
                ("assets/%2e%2e/evil.png", b"\x89PNG\r\n\x1a\nrest"),
            ],
        );

        assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafeResource);
    }

    #[test]
    fn rejects_duplicate_normalized_paths_and_case_folded_collisions() {
        for entries in [
            vec![
                ("assets/cafe\u{301}.png", b"a".as_slice()),
                ("assets/caf\u{e9}.png", b"b"),
            ],
            vec![
                ("assets/Icon.png", b"a".as_slice()),
                ("assets/icon.png", b"b"),
            ],
        ] {
            let bytes = zip_entries(CompressionMethod::Stored, &entries);
            assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafePath);
        }
    }

    #[test]
    fn rejects_symlinks_hard_link_metadata_and_special_unix_modes() {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        archive
            .add_symlink(
                "assets/link.png",
                "../outside",
                SimpleFileOptions::default(),
            )
            .unwrap();
        assert_eq!(
            reject_archive(&archive.finish().unwrap().into_inner()),
            ThemeErrorCode::UnsafePath
        );

        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let mut options = FullFileOptions::default();
        options
            .add_extra_data(0xcafe, b"theme.css".to_vec().into_boxed_slice(), true)
            .unwrap();
        archive.start_file("assets/hard.png", options).unwrap();
        archive.write_all(b"theme.css").unwrap();
        let mut hard_link = archive.finish().unwrap().into_inner();
        let marker = [0xfe, 0xca, 9, 0];
        let marker_offset = hard_link
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("central extra-field marker");
        hard_link[marker_offset..marker_offset + 2].copy_from_slice(&0x000d_u16.to_le_bytes());
        assert_eq!(reject_archive(&hard_link), ThemeErrorCode::UnsafePath);

        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = ZipWriter::new(cursor);
        let mut options = FullFileOptions::default();
        options
            .add_extra_data(0xcafe, b"theme.css".to_vec().into_boxed_slice(), false)
            .unwrap();
        archive
            .start_file("assets/local-hard.png", options)
            .unwrap();
        archive.write_all(b"theme.css").unwrap();
        let mut local_hard_link = archive.finish().unwrap().into_inner();
        let marker_offset = local_hard_link
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("local extra-field marker");
        local_hard_link[marker_offset..marker_offset + 2]
            .copy_from_slice(&0x000d_u16.to_le_bytes());
        assert_eq!(reject_archive(&local_hard_link), ThemeErrorCode::UnsafePath);

        for special_mode in [0o010644_u32, 0o020644, 0o060644, 0o140644] {
            let mut bytes = zip_entries(CompressionMethod::Stored, &[("assets/special.png", b"x")]);
            patch_headers(&mut bytes, |header, central| {
                if central {
                    header[38..42].copy_from_slice(&(special_mode << 16).to_le_bytes());
                }
            });
            assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafePath);
        }
    }

    #[test]
    fn rejects_encrypted_entries_and_unsupported_compression() {
        let mut encrypted = zip_entries(CompressionMethod::Stored, &[("manifest.json", MANIFEST)]);
        patch_headers(&mut encrypted, |header, central| {
            let flags_offset = if central { 8 } else { 6 };
            let flags = u16::from_le_bytes([header[flags_offset], header[flags_offset + 1]]) | 1;
            header[flags_offset..flags_offset + 2].copy_from_slice(&flags.to_le_bytes());
        });
        assert_eq!(reject_archive(&encrypted), ThemeErrorCode::InvalidArchive);

        let mut unsupported =
            zip_entries(CompressionMethod::Stored, &[("manifest.json", MANIFEST)]);
        patch_headers(&mut unsupported, |header, central| {
            let method_offset = if central { 10 } else { 8 };
            header[method_offset..method_offset + 2].copy_from_slice(&99_u16.to_le_bytes());
        });
        assert_eq!(reject_archive(&unsupported), ThemeErrorCode::InvalidArchive);
    }

    #[test]
    fn rejects_local_header_metadata_that_disagrees_with_the_central_directory() {
        let mutations: [fn(&mut [u8]); 5] = [
            |header| {
                let flags = u16::from_le_bytes([header[6], header[7]]) ^ (1 << 11);
                header[6..8].copy_from_slice(&flags.to_le_bytes());
            },
            |header| header[8..10].copy_from_slice(&8_u16.to_le_bytes()),
            |header| header[14..18].copy_from_slice(&0x1234_5678_u32.to_le_bytes()),
            |header| header[18..22].copy_from_slice(&1_u32.to_le_bytes()),
            |header| header[22..26].copy_from_slice(&1_u32.to_le_bytes()),
        ];
        for mutate in mutations {
            let mut bytes = minimal_package(CompressionMethod::Stored);
            let local = bytes
                .windows(4)
                .position(|window| window == b"PK\x03\x04")
                .unwrap();
            mutate(&mut bytes[local..]);
            assert_eq!(reject_archive(&bytes), ThemeErrorCode::InvalidArchive);
        }
    }

    #[test]
    fn accepts_consistent_data_descriptors_and_rejects_a_local_only_descriptor_mismatch() {
        let valid = streaming_package(CompressionMethod::Deflated);
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let source = write_source(temp.path(), &valid);
        assert!(prepare_external_theme(&source, &catalog).is_ok());

        let mut invalid = valid;
        let descriptor = invalid
            .windows(4)
            .position(|window| window == b"PK\x07\x08")
            .expect("data descriptor");
        invalid[descriptor + 4..descriptor + 8].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        assert_eq!(reject_archive(&invalid), ThemeErrorCode::InvalidArchive);
    }

    #[test]
    fn rejects_central_disk_start_and_per_entry_zip64_markers() {
        let mut disk_start = minimal_package(CompressionMethod::Stored);
        let central = disk_start
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .unwrap();
        disk_start[central + 34..central + 36].copy_from_slice(&1_u16.to_le_bytes());
        assert_eq!(reject_archive(&disk_start), ThemeErrorCode::InvalidArchive);

        for (central_field, local_field) in [(20, 18), (24, 22)] {
            let mut central_sentinel = minimal_package(CompressionMethod::Stored);
            let central = central_sentinel
                .windows(4)
                .position(|window| window == b"PK\x01\x02")
                .unwrap();
            central_sentinel[central + central_field..central + central_field + 4]
                .copy_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(
                reject_archive(&central_sentinel),
                ThemeErrorCode::InvalidArchive
            );

            let mut local_sentinel = minimal_package(CompressionMethod::Stored);
            let local = local_sentinel
                .windows(4)
                .position(|window| window == b"PK\x03\x04")
                .unwrap();
            local_sentinel[local + local_field..local + local_field + 4]
                .copy_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(
                reject_archive(&local_sentinel),
                ThemeErrorCode::InvalidArchive
            );
        }

        for central_only in [false, true] {
            let cursor = std::io::Cursor::new(Vec::new());
            let mut archive = ZipWriter::new(cursor);
            let mut options = FullFileOptions::default();
            options
                .add_extra_data(0xcafe, [0_u8; 8].to_vec().into_boxed_slice(), central_only)
                .unwrap();
            archive.start_file("manifest.json", options).unwrap();
            archive.write_all(MANIFEST).unwrap();
            let mut bytes = archive.finish().unwrap().into_inner();
            let marker = [0xfe, 0xca, 8, 0];
            let marker_offset = bytes
                .windows(marker.len())
                .position(|window| window == marker)
                .unwrap();
            bytes[marker_offset..marker_offset + 2].copy_from_slice(&1_u16.to_le_bytes());
            assert_eq!(reject_archive(&bytes), ThemeErrorCode::InvalidArchive);
        }
    }

    #[test]
    fn rejects_portable_windows_aliases_and_control_characters() {
        for name in [
            "assets/name:stream.png",
            "assets/trailing-dot./image.png",
            "assets/trailing-space /image.png",
            "assets/CON.png",
            "assets/prn.anything.png",
            "assets/AUX",
            "assets/nul.txt",
            "assets/COM1.png",
            "assets/lpt9.svg",
            "assets/CON .png",
            "assets/COM\u{00b9}.png",
            "assets/LPT\u{00b3}.svg",
            "assets/CLOCK$.png",
            "assets/control\u{001f}.png",
            "assets/delete\u{007f}.png",
        ] {
            let bytes = zip_entries(CompressionMethod::Stored, &[(name, b"x")]);
            assert_eq!(reject_archive(&bytes), ThemeErrorCode::UnsafePath, "{name}");
        }
    }

    #[test]
    fn rejects_nested_archive_extensions() {
        for name in ["assets/nested.zip", "assets/nested.theme"] {
            assert_eq!(
                reject_archive(&zip_entries(
                    CompressionMethod::Stored,
                    &[(name, b"PK\x03\x04")]
                )),
                ThemeErrorCode::UnsafePath
            );
        }
    }

    #[test]
    fn validation_failure_removes_all_staging_and_never_writes_outside() {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let outside = temp.path().join("outside.css");
        let invalid = zip_entries(
            CompressionMethod::Stored,
            &[
                ("manifest.json", MANIFEST),
                ("theme.css", b"@import 'bad';"),
            ],
        );
        let source = write_source(temp.path(), &invalid);

        let error = prepare_external_theme(&source, &catalog).unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafeResource);
        assert!(!outside.exists());
        assert_no_staging(&catalog);

        let traversal = zip_entries(CompressionMethod::Stored, &[("../outside.css", b"owned")]);
        fs::write(&source, traversal).unwrap();
        assert!(prepare_external_theme(&source, &catalog).is_err());
        assert!(!outside.exists());
        assert_no_staging(&catalog);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_staging_root_substitution_without_writing_to_the_substitute() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        let outside = temp.path().join("outside");
        fs::create_dir(&catalog).unwrap();
        fs::create_dir(&outside).unwrap();
        let source = write_source(temp.path(), &package_with_asset(CompressionMethod::Stored));
        let mut replaced = false;

        let error = prepare_external_theme_with_hooks(
            &source,
            &catalog,
            &mut || {},
            &mut |point, target| {
                if point == ExtractionHookPoint::BeforeFileCreate && !replaced {
                    let staging = target.parent().unwrap();
                    fs::rename(staging, catalog.join("moved-staging")).unwrap();
                    symlink(&outside, staging).unwrap();
                    replaced = true;
                }
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        assert_no_staging(&catalog);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_intermediate_directory_substitution_without_following_it() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        let outside = temp.path().join("outside");
        fs::create_dir(&catalog).unwrap();
        fs::create_dir(&outside).unwrap();
        let source = write_source(temp.path(), &package_with_asset(CompressionMethod::Stored));
        let mut replaced = false;

        let error = prepare_external_theme_with_hooks(
            &source,
            &catalog,
            &mut || {},
            &mut |point, target| {
                if point == ExtractionHookPoint::BeforeFileCreate
                    && target.ends_with("assets/image.png")
                    && !replaced
                {
                    let assets = target.parent().unwrap();
                    let moved = assets.parent().unwrap().join("moved-assets");
                    fs::rename(assets, moved).unwrap();
                    symlink(&outside, assets).unwrap();
                    replaced = true;
                }
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::UnsafePath);
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        assert_no_staging(&catalog);
    }

    #[test]
    fn rejects_same_inode_same_length_mutation_after_validation_for_every_file_class() {
        for relative in ["theme.css", "manifest.json", "assets/image.png"] {
            let temp = tempdir().unwrap();
            let catalog = temp.path().join("catalog");
            fs::create_dir(&catalog).unwrap();
            let source = write_source(temp.path(), &package_with_asset(CompressionMethod::Stored));

            let error = prepare_external_theme_with_hooks(
                &source,
                &catalog,
                &mut || {},
                &mut |point, root| {
                    if point == ExtractionHookPoint::AfterValidation {
                        let target = root.join(relative);
                        let mut bytes = fs::read(&target).unwrap();
                        bytes[0] ^= 0x01;
                        fs::write(target, bytes).unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();

            assert_eq!(error.code, ThemeErrorCode::UnsafePath, "{relative}");
            assert_no_staging(&catalog);
        }
    }

    #[test]
    fn staging_creation_failure_after_create_is_cleaned_without_residue() {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let source = write_source(temp.path(), &minimal_package(CompressionMethod::Stored));

        let error =
            prepare_external_theme_with_hooks(&source, &catalog, &mut || {}, &mut |point, _| {
                if point == ExtractionHookPoint::AfterStagingCreate {
                    return Err(super::io_error(std::io::Error::other(
                        "injected post-create failure",
                    )));
                }
                Ok(())
            })
            .unwrap_err();

        assert_eq!(error.code, ThemeErrorCode::Io);
        assert_no_staging(&catalog);
    }

    #[test]
    fn compression_does_not_change_the_content_fingerprint() {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let stored = write_source(temp.path(), &minimal_package(CompressionMethod::Stored));
        let stored_fingerprint = prepare_external_theme(&stored, &catalog)
            .unwrap()
            .descriptor()
            .fingerprint
            .clone();
        fs::write(&stored, minimal_package(CompressionMethod::Deflated)).unwrap();
        let deflated_fingerprint = prepare_external_theme(&stored, &catalog)
            .unwrap()
            .descriptor()
            .fingerprint
            .clone();

        assert_eq!(stored_fingerprint, deflated_fingerprint);
    }

    #[test]
    fn dropping_a_prepared_package_removes_its_owned_staging_directory() {
        let temp = tempdir().unwrap();
        let catalog = temp.path().join("catalog");
        fs::create_dir(&catalog).unwrap();
        let source = write_source(temp.path(), &minimal_package(CompressionMethod::Stored));
        let staging = match prepare_external_theme(&source, &catalog).unwrap() {
            PreparedThemeImport::ResourcePackage(package) => {
                let staging = package.validated().root.clone();
                assert!(staging.exists());
                drop(package);
                staging
            }
            PreparedThemeImport::LegacyCss(_) => panic!("expected package"),
        };

        assert!(!staging.exists());
        assert_no_staging(&catalog);
    }
}
