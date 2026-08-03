use serde_json::Value;
use tauri_plugin_dialog::DialogExt as _;

const SETTINGS_EXPORT_MAX_BYTES: usize = 16 * 1024 * 1024;
const INVALID_SETTINGS_EXPORT: &str = "invalid-settings-export";
const INVALID_SETTINGS_EXPORT_NAME: &str = "invalid-settings-export-name";
const SETTINGS_EXPORT_PATH_UNAVAILABLE: &str = "settings-export-path-unavailable";

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavedSettingsFile {
    path: String,
    name: String,
}

#[tauri::command]
pub(crate) async fn save_settings_file(
    window: tauri::Window,
    suggested_name: String,
    contents: String,
) -> Result<Option<SavedSettingsFile>, String> {
    validate_settings_export_name(&suggested_name)?;
    validate_settings_export_contents(&contents)?;

    let selected = window
        .dialog()
        .file()
        .set_parent(&window)
        .set_file_name(suggested_name)
        .add_filter("QingYu settings", &["json"])
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| SETTINGS_EXPORT_PATH_UNAVAILABLE.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| SETTINGS_EXPORT_PATH_UNAVAILABLE.to_string())?
        .to_string();
    let path = path.to_string_lossy().to_string();

    crate::text_file::write_text_file(path.clone(), contents)?;

    Ok(Some(SavedSettingsFile { path, name }))
}

fn validate_settings_export_name(name: &str) -> Result<(), &'static str> {
    let valid = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().count() <= 255
        && !name
            .chars()
            .any(|character| character.is_control() || character == '/' || character == '\\')
        && name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("json"));
    if valid {
        Ok(())
    } else {
        Err(INVALID_SETTINGS_EXPORT_NAME)
    }
}

fn validate_settings_export_contents(contents: &str) -> Result<(), &'static str> {
    if contents.len() > SETTINGS_EXPORT_MAX_BYTES {
        return Err(INVALID_SETTINGS_EXPORT);
    }
    let value: Value = serde_json::from_str(contents).map_err(|_| INVALID_SETTINGS_EXPORT)?;
    let object = value.as_object().ok_or(INVALID_SETTINGS_EXPORT)?;
    let valid = object.get("exportedAt").and_then(Value::as_str).is_some()
        && object.get("format").and_then(Value::as_str) == Some("markra-settings")
        && object.get("settings").and_then(Value::as_object).is_some()
        && object.get("version").and_then(Value::as_u64) == Some(3);
    if valid {
        Ok(())
    } else {
        Err(INVALID_SETTINGS_EXPORT)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_settings_export_contents, validate_settings_export_name, INVALID_SETTINGS_EXPORT,
        INVALID_SETTINGS_EXPORT_NAME, SETTINGS_EXPORT_MAX_BYTES,
    };

    const VALID_SETTINGS_EXPORT: &str = r#"{
        "exportedAt":"2026-08-03T13:39:05.523Z",
        "format":"markra-settings",
        "settings":{},
        "version":3
    }"#;

    #[test]
    fn settings_export_validation_accepts_the_current_envelope() {
        assert_eq!(
            validate_settings_export_contents(VALID_SETTINGS_EXPORT),
            Ok(())
        );
    }

    #[test]
    fn settings_export_validation_rejects_non_settings_json() {
        for contents in [
            "not json",
            r#"{"format":"markra-settings","settings":[],"version":3}"#,
            r#"{"format":"other","settings":{},"version":3}"#,
            r#"{"format":"markra-settings","settings":{},"version":2}"#,
        ] {
            assert_eq!(
                validate_settings_export_contents(contents),
                Err(INVALID_SETTINGS_EXPORT)
            );
        }
    }

    #[test]
    fn settings_export_validation_rejects_oversized_contents() {
        let oversized = "x".repeat(SETTINGS_EXPORT_MAX_BYTES + 1);

        assert_eq!(
            validate_settings_export_contents(&oversized),
            Err(INVALID_SETTINGS_EXPORT)
        );
    }

    #[test]
    fn settings_export_name_validation_accepts_only_a_json_file_name() {
        assert_eq!(
            validate_settings_export_name("markra-settings.json"),
            Ok(())
        );
        assert_eq!(validate_settings_export_name("SETTINGS.JSON"), Ok(()));
        for name in [
            "",
            ".",
            "..",
            "settings",
            "settings.txt",
            "../settings.json",
            "a/b.json",
        ] {
            assert_eq!(
                validate_settings_export_name(name),
                Err(INVALID_SETTINGS_EXPORT_NAME)
            );
        }
    }
}
