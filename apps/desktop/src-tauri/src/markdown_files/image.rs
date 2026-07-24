use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::asset::allow_asset_directory;
#[cfg(desktop)]
use super::path::{is_markdown_tree_asset_file, path_to_string};
use super::resource_writer::{save_project_resource_bytes, save_standalone_resource_with_writer};
use super::types::ClipboardImageFile;
#[cfg(desktop)]
use super::types::MarkdownImageFile;

fn clipboard_image_extension(mime_type: &str) -> Result<&'static str, String> {
    match normalized_clipboard_image_mime_type(mime_type)? {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/gif" => Ok("gif"),
        "image/webp" => Ok("webp"),
        "image/avif" => Ok("avif"),
        "image/bmp" => Ok("bmp"),
        "image/svg+xml" => Ok("svg"),
        _ => Err("Clipboard image type is not supported".to_string()),
    }
}

fn normalized_clipboard_image_mime_type(mime_type: &str) -> Result<&'static str, String> {
    let normalized = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "image/png" => Ok("image/png"),
        "image/jpeg" | "image/jpg" => Ok("image/jpeg"),
        "image/gif" => Ok("image/gif"),
        "image/webp" => Ok("image/webp"),
        "image/avif" => Ok("image/avif"),
        "image/bmp" => Ok("image/bmp"),
        "image/svg+xml" => Ok("image/svg+xml"),
        _ => Err("Clipboard image type is not supported".to_string()),
    }
}

fn bytes_start_with(bytes: &[u8], signature: &[u8]) -> bool {
    bytes.get(..signature.len()) == Some(signature)
}

fn avif_signature(bytes: &[u8]) -> bool {
    if bytes.len() < 16 || bytes.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let declared_size = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let end = if declared_size >= 16 && declared_size <= bytes.len() {
        declared_size
    } else {
        bytes.len()
    };
    (8..end.saturating_sub(3)).step_by(4).any(|offset| {
        offset != 12 && matches!(bytes.get(offset..offset + 4), Some(b"avif") | Some(b"avis"))
    })
}

fn utf8_svg_signature(bytes: &[u8]) -> bool {
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut source = source.trim_start_matches('\u{feff}').trim_start();
    loop {
        if source.starts_with("<?xml") {
            let Some(end) = source.find("?>") else {
                return false;
            };
            source = source[end + 2..].trim_start();
            continue;
        }
        if source.starts_with("<!--") {
            let Some(end) = source.find("-->") else {
                return false;
            };
            source = source[end + 3..].trim_start();
            continue;
        }
        break;
    }
    let Some(prefix) = source.get(..4) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case("<svg") {
        return false;
    }
    source
        .as_bytes()
        .get(4)
        .is_some_and(|byte| *byte == b'>' || byte.is_ascii_whitespace())
}

fn detected_image_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes_start_with(bytes, b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes_start_with(bytes, b"\xff\xd8\xff") {
        return Some("image/jpeg");
    }
    if bytes_start_with(bytes, b"GIF87a") || bytes_start_with(bytes, b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes_start_with(bytes, b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if bytes_start_with(bytes, b"BM") {
        return Some("image/bmp");
    }
    if avif_signature(bytes) {
        return Some("image/avif");
    }
    if utf8_svg_signature(bytes) {
        return Some("image/svg+xml");
    }
    None
}

fn validate_clipboard_image_bytes(mime_type: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("Clipboard image is empty".to_string());
    }
    let expected = normalized_clipboard_image_mime_type(mime_type)?;
    let Some(detected) = detected_image_mime_type(bytes) else {
        return Err("Clipboard image bytes are not a supported image".to_string());
    };
    if detected != expected {
        return Err("Clipboard image MIME type does not match image bytes".to_string());
    }
    Ok(())
}

#[cfg(desktop)]
fn markdown_image_mime_type(path: &Path) -> Result<&'static str, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        "avif" => Ok("image/avif"),
        "bmp" => Ok("image/bmp"),
        "svg" => Ok("image/svg+xml"),
        _ => Err("Markdown image type is not supported".to_string()),
    }
}

#[cfg(desktop)]
fn read_local_image_file_for_import(path: String) -> Result<MarkdownImageFile, String> {
    let canonical_path = PathBuf::from(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;

    if !canonical_path.is_file() || !is_markdown_tree_asset_file(&canonical_path) {
        return Err("Path is not a supported image".to_string());
    }

    Ok(MarkdownImageFile {
        bytes: fs::read(&canonical_path).map_err(|error| error.to_string())?,
        mime_type: markdown_image_mime_type(&canonical_path)?.to_string(),
        path: path_to_string(&canonical_path),
    })
}

fn normalize_clipboard_image_folder(folder: &str) -> Result<PathBuf, String> {
    let normalized = folder.trim().replace('\\', "/");
    if normalized == "." {
        return Ok(PathBuf::new());
    }

    let candidate = Path::new(&normalized);
    if normalized.is_empty() || candidate.is_absolute() {
        return Err("Clipboard image folder must be relative".to_string());
    }

    let mut target = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => target.push(part),
            Component::CurDir => {}
            _ => return Err("Clipboard image folder cannot leave the current folder".to_string()),
        }
    }

    if target.as_os_str().is_empty() {
        return Err("Clipboard image folder is invalid".to_string());
    }

    Ok(target)
}

fn requested_clipboard_image_stem(file_name: &str) -> Result<String, String> {
    let trimmed = file_name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || matches!(trimmed, "." | "..")
    {
        return Err("Clipboard image file name is invalid".to_string());
    }

    let stem = trimmed
        .rsplit_once('.')
        .map_or(trimmed, |(stem, _)| stem)
        .trim();
    if stem.is_empty() || matches!(stem, "." | "..") {
        return Err("Clipboard image file name is invalid".to_string());
    }

    Ok(stem.to_string())
}

fn clipboard_image_file_name(
    extension: &str,
    attempt: usize,
    requested_file_name: Option<&str>,
) -> Result<String, String> {
    if let Some(file_name) = requested_file_name {
        let stem = requested_clipboard_image_stem(file_name)?;
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{}", attempt + 1)
        };

        return Ok(format!("{stem}{suffix}.{extension}"));
    }

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let suffix = if attempt == 0 {
        String::new()
    } else {
        format!("-{}", attempt + 1)
    };

    Ok(format!("pasted-image-{millis}{suffix}.{extension}"))
}

fn save_clipboard_image_file(
    document_path: String,
    folder: String,
    mime_type: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<ClipboardImageFile, String> {
    save_clipboard_image_file_with_writer(
        document_path,
        folder,
        mime_type,
        bytes,
        file_name,
        allow_root_assets,
        forbid_root_assets,
        |target, contents| {
            let mut source = io::Cursor::new(contents);
            io::copy(&mut source, target).map(|_| ())
        },
    )
}

fn save_clipboard_image_file_with_writer(
    document_path: String,
    folder: String,
    mime_type: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    write_contents: impl FnOnce(&mut fs::File, &[u8]) -> io::Result<()>,
) -> Result<ClipboardImageFile, String> {
    validate_clipboard_image_bytes(&mime_type, &bytes)?;
    let extension = clipboard_image_extension(&mime_type)?;
    let target_name = clipboard_image_file_name(extension, 0, file_name.as_deref())?;
    let saved = save_standalone_resource_with_writer(
        document_path,
        normalize_clipboard_image_folder(&folder)?,
        target_name,
        allow_root_assets,
        forbid_root_assets,
        move |target| write_contents(target, &bytes),
    )?;
    Ok(ClipboardImageFile {
        relative_path: saved.relative_path,
    })
}

fn save_project_clipboard_image_file(
    document_path: String,
    project_root_path: String,
    mime_type: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
    source_path: Option<String>,
    allow_root_assets: impl FnOnce(&Path) -> Result<(), String>,
    forbid_root_assets: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<ClipboardImageFile, String> {
    validate_clipboard_image_bytes(&mime_type, &bytes)?;
    let extension = clipboard_image_extension(&mime_type)?;
    let target_name = clipboard_image_file_name(extension, 0, file_name.as_deref())?;
    let saved = save_project_resource_bytes(
        document_path,
        project_root_path,
        bytes,
        target_name,
        source_path,
        allow_root_assets,
        forbid_root_assets,
    )?;

    Ok(ClipboardImageFile {
        relative_path: saved.relative_path,
    })
}

fn forbid_asset_directory(app: &tauri::AppHandle, root: &Path) -> Result<(), String> {
    use tauri::Manager;

    app.asset_protocol_scope()
        .forbid_directory(root, true)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn save_clipboard_image(
    app: tauri::AppHandle,
    document_path: String,
    folder: String,
    mime_type: String,
    bytes: Vec<u8>,
    file_name: Option<String>,
    project_root_path: Option<String>,
    source_path: Option<String>,
) -> Result<ClipboardImageFile, String> {
    if let Some(project_root_path) = project_root_path {
        return save_project_clipboard_image_file(
            document_path,
            project_root_path,
            mime_type,
            bytes,
            file_name,
            source_path,
            |root| allow_asset_directory(&app, root),
            |root| forbid_asset_directory(&app, root),
        );
    }

    save_clipboard_image_file(
        document_path,
        folder,
        mime_type,
        bytes,
        file_name,
        |root| allow_asset_directory(&app, root),
        |root| forbid_asset_directory(&app, root),
    )
}

#[cfg(desktop)]
#[tauri::command]
pub(crate) fn read_local_image_file(path: String) -> Result<MarkdownImageFile, String> {
    read_local_image_file_for_import(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_png() -> Vec<u8> {
        vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]
    }

    fn valid_jpeg() -> Vec<u8> {
        vec![0xff, 0xd8, 0xff, 0xdb]
    }

    fn project_fixture(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "qingyu-project-image-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("notes").join("day.md");
        fs::create_dir_all(note.parent().expect("note should have a parent"))
            .expect("note directory should be created");
        fs::write(&note, "# Day").expect("note should be created");
        (root, note)
    }

    #[test]
    fn saves_primary_workspace_images_in_fixed_lowercase_root_assets() {
        let (root, note) = project_fixture("root-assets");

        let saved = save_project_clipboard_image_file(
            note.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("diagram.png".to_string()),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("project image should be saved");

        assert_eq!(saved.relative_path, "../assets/diagram.png");
        assert_eq!(
            fs::read(root.join("assets").join("diagram.png"))
                .expect("saved image should be readable"),
            valid_png()
        );
        let root_entries = fs::read_dir(&root)
            .expect("workspace root should be readable")
            .map(|entry| {
                entry
                    .expect("workspace entry should be readable")
                    .file_name()
            })
            .collect::<Vec<_>>();
        assert!(root_entries.iter().any(|name| name == "assets"));
        assert!(!root_entries.iter().any(|name| name == "Assets"));
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn rejects_primary_workspace_images_for_documents_outside_the_root() {
        let (root, _note) = project_fixture("outside-root");
        let (outside_root, outside_note) = project_fixture("outside-note");

        let error = save_project_clipboard_image_file(
            outside_note.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("diagram.png".to_string()),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect_err("outside documents must be rejected");

        assert!(error.contains("inside the project"));
        fs::remove_dir_all(root).expect("fixture should be removed");
        fs::remove_dir_all(outside_root).expect("fixture should be removed");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_project_assets_directory() {
        let (root, note) = project_fixture("symlink-assets");
        let outside = std::env::temp_dir().join(format!(
            "qingyu-project-image-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&outside).expect("outside directory should be created");
        std::os::unix::fs::symlink(&outside, root.join("assets"))
            .expect("assets symlink should be created");

        assert!(save_project_clipboard_image_file(
            note.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("diagram.png".to_string()),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .is_err());

        fs::remove_dir_all(root).expect("fixture should be removed");
        fs::remove_dir_all(outside).expect("outside directory should be removed");
    }

    #[test]
    fn references_existing_project_asset_without_copying() {
        let (root, note) = project_fixture("existing-asset");
        let assets = root.join("assets");
        let source = assets.join("existing.png");
        fs::create_dir_all(&assets).expect("assets directory should be created");
        fs::write(&source, valid_png()).expect("existing image should be created");

        let saved = save_project_clipboard_image_file(
            note.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("existing.png".to_string()),
            Some(source.to_string_lossy().to_string()),
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("existing asset should be referenced");

        assert_eq!(saved.relative_path, "../assets/existing.png");
        assert_eq!(
            fs::read_dir(&assets)
                .expect("assets should be readable")
                .count(),
            1
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn saves_clipboard_images_below_the_current_markdown_file_directory() {
        let root = std::env::temp_dir().join(format!(
            "markra-clipboard-image-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("note.md");

        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Note").expect("markdown file should be created");

        let saved = save_clipboard_image_file(
            note.to_string_lossy().to_string(),
            "assets/screenshots".to_string(),
            "image/png".to_string(),
            valid_png(),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("clipboard image should be saved");

        assert!(saved
            .relative_path
            .starts_with("assets/screenshots/pasted-image-"));
        assert!(saved.relative_path.ends_with(".png"));
        assert_eq!(
            fs::read(root.join(saved.relative_path)).expect("saved image should be readable"),
            valid_png()
        );

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn saves_clipboard_images_with_the_requested_file_name() {
        let root = std::env::temp_dir().join(format!(
            "markra-clipboard-image-name-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("note.md");

        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Note").expect("markdown file should be created");

        let saved = save_clipboard_image_file(
            note.to_string_lossy().to_string(),
            "assets".to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("diagram-from-rule.png".to_string()),
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("clipboard image should use the requested name");

        assert_eq!(saved.relative_path, "assets/diagram-from-rule.png");
        assert_eq!(
            fs::read(root.join(saved.relative_path)).expect("saved image should be readable"),
            valid_png()
        );

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn saves_svg_clipboard_images_with_svg_extension() {
        assert_eq!(
            clipboard_image_extension("image/svg+xml").expect("SVG images should be supported"),
            "svg"
        );
    }

    #[test]
    fn reads_supported_local_images_for_import() {
        let root = std::env::temp_dir().join(format!(
            "markra-read-local-image-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test folder should be created");
        let image = root.join("Local Diagram.png");
        fs::write(&image, valid_png()).expect("image file should be created");

        let read = read_local_image_file_for_import(image.to_string_lossy().to_string())
            .expect("local image should be readable for import");

        assert_eq!(read.bytes, valid_png());
        assert_eq!(read.mime_type, "image/png");
        assert_eq!(
            read.path,
            path_to_string(&image.canonicalize().expect("image should canonicalize"))
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_clipboard_image_folders_outside_the_current_markdown_file_directory() {
        let root = std::env::temp_dir().join(format!(
            "markra-clipboard-image-boundary-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos()
        ));
        let note = root.join("note.md");

        fs::create_dir_all(&root).expect("test folder should be created");
        fs::write(&note, "# Note").expect("markdown file should be created");

        assert!(save_clipboard_image_file(
            note.to_string_lossy().to_string(),
            "../outside".to_string(),
            "image/png".to_string(),
            valid_png(),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .is_err());

        fs::remove_dir_all(root).expect("test tree should be removed");
    }

    #[test]
    fn validates_supported_image_signatures_against_mime_type() {
        let cases = [
            ("image/png", valid_png()),
            ("image/jpeg", valid_jpeg()),
            ("image/gif", b"GIF89a".to_vec()),
            ("image/webp", b"RIFF\x04\x00\x00\x00WEBP".to_vec()),
            ("image/bmp", b"BM\x1a\x00\x00\x00".to_vec()),
            (
                "image/avif",
                b"\x00\x00\x00\x18ftypavif\x00\x00\x00\x00avifmif1".to_vec(),
            ),
            (
                "image/svg+xml",
                b"\xef\xbb\xbf <?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".to_vec(),
            ),
        ];

        for (mime_type, bytes) in cases {
            validate_clipboard_image_bytes(mime_type, &bytes)
                .unwrap_or_else(|error| panic!("{mime_type} should be valid: {error}"));
        }
    }

    #[test]
    fn rejects_mime_and_signature_disagreement_before_creating_assets() {
        let (root, note) = project_fixture("mime-mismatch");

        let error = save_project_clipboard_image_file(
            note.to_string_lossy().to_string(),
            root.to_string_lossy().to_string(),
            "image/jpeg".to_string(),
            valid_png(),
            Some("diagram.jpg".to_string()),
            None,
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect_err("a mismatched signature must be rejected");

        assert!(error.contains("does not match"));
        assert!(!root.join("assets").exists());
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn publishes_complete_standalone_image_bytes_only_after_writer_success() {
        use std::io::Write;

        let (root, note) = project_fixture("complete-standalone");
        let bytes = valid_png();
        let final_path = root.join("notes/assets/diagram.png");
        let saved = save_clipboard_image_file_with_writer(
            note.to_string_lossy().to_string(),
            "assets".to_string(),
            "image/png".to_string(),
            bytes.clone(),
            Some("diagram.png".to_string()),
            |_| Ok(()),
            |_| Ok(()),
            |target, contents| {
                target.write_all(&contents[..4])?;
                assert!(!final_path.exists(), "partial bytes must stay in staging");
                target.write_all(&contents[4..])
            },
        )
        .expect("complete image should be published");

        assert_eq!(saved.relative_path, "assets/diagram.png");
        assert_eq!(
            fs::read(final_path).expect("published image should be readable"),
            bytes
        );
        assert!(fs::read_dir(root.join("notes/assets"))
            .expect("assets should be readable")
            .all(|entry| !entry
                .expect("entry should be readable")
                .file_name()
                .to_string_lossy()
                .starts_with(".qingyu-resource-")));
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn removes_image_staging_and_final_file_after_injected_write_failure() {
        use std::io::Write;

        let (root, note) = project_fixture("failed-standalone");
        let error = save_clipboard_image_file_with_writer(
            note.to_string_lossy().to_string(),
            "assets".to_string(),
            "image/png".to_string(),
            valid_png(),
            Some("diagram.png".to_string()),
            |_| Ok(()),
            |_| Ok(()),
            |target, contents| {
                target.write_all(&contents[..4])?;
                Err(std::io::Error::other("injected image write failure"))
            },
        )
        .expect_err("writer failure should abort publication");

        assert!(error.contains("injected image write failure"));
        let assets = root.join("notes/assets");
        assert!(!assets.join("diagram.png").exists());
        assert_eq!(
            fs::read_dir(&assets)
                .expect("assets should be readable after cleanup")
                .count(),
            0
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }

    #[test]
    fn suffixes_image_name_collisions_without_clobbering() {
        let (root, note) = project_fixture("collision");
        let save = || {
            save_project_clipboard_image_file(
                note.to_string_lossy().to_string(),
                root.to_string_lossy().to_string(),
                "image/png".to_string(),
                valid_png(),
                Some("diagram.png".to_string()),
                None,
                |_| Ok(()),
                |_| Ok(()),
            )
        };

        assert_eq!(
            save().expect("first image should save").relative_path,
            "../assets/diagram.png"
        );
        assert_eq!(
            save().expect("collision should save").relative_path,
            "../assets/diagram-2.png"
        );
        assert_eq!(
            fs::read(root.join("assets/diagram.png")).unwrap(),
            valid_png()
        );
        assert_eq!(
            fs::read(root.join("assets/diagram-2.png")).unwrap(),
            valid_png()
        );
        fs::remove_dir_all(root).expect("fixture should be removed");
    }
}
