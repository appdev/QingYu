use std::fs;
use std::path::{Path, PathBuf};

use super::path::normalize_markdown_tree_single_file_name;
use super::types::MarkdownTemplateFile;
use tauri::Manager;

fn normalize_markdown_template_file_name(file_name: &str) -> Result<String, String> {
    let trimmed_name = normalize_markdown_tree_single_file_name(file_name)?;
    let candidate = Path::new(&trimmed_name);

    if candidate
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return Ok(trimmed_name);
    }

    Err("Template file must use .md".to_string())
}

fn normalize_new_markdown_template_file_name(file_name: &str) -> Result<String, String> {
    let normalized_name = normalize_markdown_template_file_name(file_name)?;
    if file_name != file_name.trim() {
        return Err("File name is not portable across supported platforms".to_string());
    }
    qingyu_kernel::contract::DocumentName::parse(normalized_name.clone())
        .map(|_| normalized_name)
        .map_err(|_| "File name is not portable across supported platforms".to_string())
}

fn markdown_template_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("templates"))
        .map_err(|error| error.to_string())
}

fn markdown_template_file_path(app: &tauri::AppHandle, file_name: &str) -> Result<PathBuf, String> {
    Ok(markdown_template_dir(app)?.join(normalize_markdown_template_file_name(file_name)?))
}

#[tauri::command]
pub(crate) fn read_markdown_template_file(
    app: tauri::AppHandle,
    file_name: String,
) -> Result<MarkdownTemplateFile, String> {
    let path = markdown_template_file_path(&app, &file_name)?;
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;

    Ok(MarkdownTemplateFile { contents })
}

#[tauri::command]
pub(crate) fn write_markdown_template_file(
    app: tauri::AppHandle,
    file_name: String,
    contents: String,
) -> Result<(), String> {
    let dir = markdown_template_dir(&app)?;
    write_markdown_template_file_in_dir(&dir, &file_name, &contents)
}

fn write_markdown_template_file_in_dir(
    dir: &Path,
    file_name: &str,
    contents: &str,
) -> Result<(), String> {
    let normalized_file_name = normalize_new_markdown_template_file_name(file_name)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::write(dir.join(normalized_file_name), contents).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn delete_markdown_template_file(
    app: tauri::AppHandle,
    file_name: String,
) -> Result<(), String> {
    let path = markdown_template_file_path(&app, &file_name)?;
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_markdown_template_file_names() {
        assert_eq!(
            normalize_markdown_template_file_name(" standup.md ")
                .expect("template file name should normalize"),
            "standup.md"
        );
        assert!(normalize_markdown_template_file_name("../standup.md").is_err());
        assert!(normalize_markdown_template_file_name("standup.markdown").is_err());
        assert!(normalize_markdown_template_file_name("standup").is_err());
    }

    #[test]
    fn template_source_normalization_keeps_legacy_nonportable_names_addressable() {
        assert_eq!(
            normalize_markdown_template_file_name(" CON.md ")
                .expect("legacy template lookup should stay structural"),
            "CON.md"
        );
        assert_eq!(
            normalize_markdown_template_file_name("bad:name.md")
                .expect("legacy template lookup should not enforce portability"),
            "bad:name.md"
        );
    }

    #[test]
    fn template_writes_reject_nonportable_final_names_without_artifacts() {
        let root = tempfile::tempdir().expect("test root should be created");
        let template_dir = root.path().join("templates");
        let overlong_name = format!("{}.md", "x".repeat(253));
        let cases = [
            ("CON.md".to_string(), "CON.md".to_string()),
            ("AUX.md".to_string(), "AUX.md".to_string()),
            ("bad:name.md".to_string(), "bad:name.md".to_string()),
            ("bad?.md".to_string(), "bad?.md".to_string()),
            ("trailing.md.".to_string(), "trailing.md.".to_string()),
            ("trailing.md ".to_string(), "trailing.md".to_string()),
            (overlong_name.clone(), overlong_name),
        ];

        for (requested_name, final_name) in cases {
            let result = write_markdown_template_file_in_dir(
                &template_dir,
                &requested_name,
                "must not be written",
            );

            assert!(result.is_err(), "{requested_name:?} should be rejected");
            assert!(
                !template_dir.join(&final_name).exists(),
                "rejected target {final_name:?} must not exist"
            );
            assert!(
                !template_dir.exists(),
                "rejected writes must not create the template directory"
            );
        }
    }
}
