use std::collections::BTreeSet;

use cssparser::{Parser, ParserInput};
use cssparser_color::Color;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use super::{ThemeAppearance, ThemeError, ThemeErrorCode, ThemePreview};

pub(crate) const MAX_THEME_ID_CHARS: usize = 64;
pub(crate) const MAX_THEME_NAME_CHARS: usize = 120;
pub(crate) const MAX_THEME_AUTHOR_CHARS: usize = 120;
pub(crate) const MAX_THEME_VERSION_CHARS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ThemeManifest {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) appearance: ThemeAppearance,
    pub(crate) entry: String,
    pub(crate) author: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) preview: ThemePreview,
    #[serde(default)]
    pub(crate) license_files: Vec<String>,
}

pub(crate) fn parse_theme_manifest(bytes: &[u8]) -> Result<ThemeManifest, ThemeError> {
    let mut manifest: ThemeManifest = serde_json::from_slice(bytes).map_err(|_| {
        invalid_manifest("Theme manifest must be valid UTF-8 JSON using the version 1 schema.")
    })?;

    if manifest.schema_version != 1 {
        return Err(invalid_manifest("Theme manifest schemaVersion must be 1."));
    }
    if manifest.entry != "theme.css" {
        return Err(invalid_manifest(
            "Theme manifest entry must be theme.css in schema version 1.",
        ));
    }
    if !valid_theme_id(&manifest.id) {
        return Err(invalid_manifest(
            "Theme manifest ID is invalid or reserved.",
        ));
    }

    manifest.name = normalize_bounded_text(&manifest.name, MAX_THEME_NAME_CHARS)
        .ok_or_else(|| invalid_manifest("Theme manifest name is empty or too long."))?;
    manifest.author = normalize_optional_bounded_text(
        manifest.author.as_deref(),
        MAX_THEME_AUTHOR_CHARS,
        "author",
    )?;
    manifest.version = normalize_optional_bounded_text(
        manifest.version.as_deref(),
        MAX_THEME_VERSION_CHARS,
        "version",
    )?;
    manifest.preview = ThemePreview {
        accent: normalize_preview_color(&manifest.preview.accent)
            .ok_or_else(|| invalid_manifest("Theme preview accent color is invalid."))?,
        background: normalize_preview_color(&manifest.preview.background)
            .ok_or_else(|| invalid_manifest("Theme preview background color is invalid."))?,
        panel: normalize_preview_color(&manifest.preview.panel)
            .ok_or_else(|| invalid_manifest("Theme preview panel color is invalid."))?,
        text: normalize_preview_color(&manifest.preview.text)
            .ok_or_else(|| invalid_manifest("Theme preview text color is invalid."))?,
    };
    manifest.license_files = normalize_license_files(&manifest.license_files)?;

    Ok(manifest)
}

pub(crate) fn valid_theme_id(id: &str) -> bool {
    if matches!(id, "light" | "dark")
        || id.starts_with("qingyu-")
        || id.is_empty()
        || id.len() > MAX_THEME_ID_CHARS
    {
        return false;
    }

    id.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || (index > 0 && byte == b'-')
    })
}

pub(crate) fn normalize_bounded_text(value: &str, max_chars: usize) -> Option<String> {
    let normalized: String = value.trim().nfc().collect();
    if normalized.is_empty() || normalized.chars().count() > max_chars {
        return None;
    }
    Some(normalized)
}

pub(crate) fn parse_theme_appearance(value: &str) -> Option<ThemeAppearance> {
    match value {
        "light" => Some(ThemeAppearance::Light),
        "dark" => Some(ThemeAppearance::Dark),
        _ => None,
    }
}

pub(crate) fn normalize_preview_color(value: &str) -> Option<String> {
    let value = value.trim();
    if !is_nontransparent_css_color(value) {
        return None;
    }
    Some(value.to_string())
}

fn normalize_optional_bounded_text(
    value: Option<&str>,
    max_chars: usize,
    field: &str,
) -> Result<Option<String>, ThemeError> {
    value
        .map(|value| {
            normalize_bounded_text(value, max_chars).ok_or_else(|| {
                invalid_manifest(format!("Theme manifest {field} is empty or too long."))
            })
        })
        .transpose()
}

fn normalize_license_files(values: &[String]) -> Result<Vec<String>, ThemeError> {
    let mut unique = BTreeSet::new();
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value: String = value.nfc().collect();
        if !valid_license_path(&value) || !unique.insert(value.clone()) {
            return Err(invalid_manifest(
                "Theme manifest licenseFiles entries must be unique safe paths below licenses/.",
            ));
        }
        normalized.push(value);
    }
    Ok(normalized)
}

fn valid_license_path(value: &str) -> bool {
    if value.contains(['\0', '\\']) || value.starts_with('/') {
        return false;
    }
    let segments: Vec<&str> = value.split('/').collect();
    if segments.len() < 2
        || segments[0] != "licenses"
        || segments
            .iter()
            .any(|segment| segment.is_empty() || matches!(*segment, "." | ".."))
    {
        return false;
    }
    matches!(
        segments.last().and_then(|name| name.rsplit_once('.')),
        Some((stem, "txt" | "md")) if !stem.is_empty()
    )
}

fn is_nontransparent_css_color(value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let Ok(color) = parser.parse_entirely(Color::parse) else {
        return false;
    };
    match color {
        Color::CurrentColor => false,
        Color::Rgba(color) => color.alpha > 0.0,
        Color::Hsl(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::Hwb(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::Lab(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::Lch(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::Oklab(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::Oklch(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
        Color::ColorFunction(color) => color.alpha.is_some_and(|alpha| alpha > 0.0),
    }
}

fn invalid_manifest(message: impl Into<String>) -> ThemeError {
    ThemeError::new(ThemeErrorCode::InvalidManifest, message)
}

#[cfg(test)]
mod tests {
    use super::parse_theme_manifest;
    use crate::themes::{ThemeAppearance, ThemeErrorCode};
    use serde_json::{json, Value};

    fn approved_manifest() -> Value {
        json!({
            "schemaVersion": 1,
            "id": "drake-light",
            "name": "Drake Light",
            "appearance": "light",
            "entry": "theme.css",
            "author": "liangjingkanji",
            "version": "2.9.6",
            "preview": {
                "background": "#ffffff",
                "panel": "#f6f8fa",
                "text": "#333333",
                "accent": "#e95f59"
            },
            "licenseFiles": [
                "licenses/THEME-LICENSE.txt",
                "licenses/FONT-LICENSE.txt"
            ]
        })
    }

    fn parse(value: &Value) -> Result<super::ThemeManifest, crate::themes::ThemeError> {
        parse_theme_manifest(serde_json::to_string(value).unwrap().as_bytes())
    }

    #[test]
    fn parses_the_approved_version_one_manifest() {
        let manifest = parse(&approved_manifest()).unwrap();

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.id, "drake-light");
        assert_eq!(manifest.name, "Drake Light");
        assert_eq!(manifest.appearance, ThemeAppearance::Light);
        assert_eq!(manifest.entry, "theme.css");
        assert_eq!(manifest.author.as_deref(), Some("liangjingkanji"));
        assert_eq!(manifest.version.as_deref(), Some("2.9.6"));
        assert_eq!(manifest.preview.background, "#ffffff");
        assert_eq!(
            manifest.license_files,
            ["licenses/THEME-LICENSE.txt", "licenses/FONT-LICENSE.txt"]
        );
    }

    #[test]
    fn rejects_missing_required_fields_and_invalid_schema_or_entry() {
        for field in [
            "schemaVersion",
            "id",
            "name",
            "appearance",
            "entry",
            "preview",
        ] {
            let mut value = approved_manifest();
            value.as_object_mut().unwrap().remove(field);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "field {field}"
            );
        }

        for schema_version in [0, 2] {
            let mut value = approved_manifest();
            value["schemaVersion"] = json!(schema_version);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest
            );
        }

        for entry in ["", "./theme.css", "styles/theme.css", "Theme.css"] {
            let mut value = approved_manifest();
            value["entry"] = json!(entry);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "entry {entry}"
            );
        }
    }

    #[test]
    fn rejects_unknown_root_and_preview_fields() {
        let mut root = approved_manifest();
        root["unexpected"] = json!(true);
        assert_eq!(
            parse(&root).unwrap_err().code,
            ThemeErrorCode::InvalidManifest
        );

        let mut preview = approved_manifest();
        preview["preview"]["unexpected"] = json!(true);
        assert_eq!(
            parse(&preview).unwrap_err().code,
            ThemeErrorCode::InvalidManifest
        );
    }

    #[test]
    fn rejects_reserved_or_malformed_ids() {
        for id in [
            "",
            "light",
            "dark",
            "qingyu-owned",
            "Upper",
            "-leading",
            "has_underscore",
            "has space",
        ] {
            let mut value = approved_manifest();
            value["id"] = json!(id);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "id {id}"
            );
        }

        let mut too_long = approved_manifest();
        too_long["id"] = json!("a".repeat(65));
        assert_eq!(
            parse(&too_long).unwrap_err().code,
            ThemeErrorCode::InvalidManifest
        );
    }

    #[test]
    fn rejects_invalid_or_transparent_preview_colors() {
        for color in [
            "",
            "not-a-color",
            "transparent",
            "#00000000",
            "rgba(0, 0, 0, 0)",
            "rgb(0 0 0 / 0)",
        ] {
            for field in ["background", "panel", "text", "accent"] {
                let mut value = approved_manifest();
                value["preview"][field] = json!(color);
                assert_eq!(
                    parse(&value).unwrap_err().code,
                    ThemeErrorCode::InvalidManifest,
                    "{field}: {color}"
                );
            }
        }
    }

    #[test]
    fn rejects_malformed_functional_preview_colors() {
        for color in [
            "rgb(nonsense)",
            "rgb(10 20)",
            "hsl(blue 50% 50%)",
            "hwb(0 10%)",
            "lab(50 nonsense 20)",
            "lch(50 20)",
            "oklab(0.5 0.1)",
            "oklch(0.5 0.1)",
            "color(not-a-color-space 0 0 0)",
        ] {
            let mut value = approved_manifest();
            value["preview"]["accent"] = json!(color);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "color {color}"
            );
        }
    }

    #[test]
    fn rejects_all_css_zero_alpha_preview_colors() {
        for color in [
            "rgba(0, 0, 0, 0.0)",
            "rgba(0, 0, 0, 0e2)",
            "rgb(0 0 0 / 0%)",
            "rgb(0 0 0 / -10%)",
            "rgb(0 0 0 / none)",
            "hsl(0 0% 0% / 0)",
            "hwb(0 0% 0% / -1)",
            "lab(0 0 0 / 0%)",
            "lch(0 0 0 / 0.000)",
            "oklab(0 0 0 / 0)",
            "oklch(0 0 0 / 0%)",
            "color(srgb 0 0 0 / 0)",
        ] {
            let mut value = approved_manifest();
            value["preview"]["accent"] = json!(color);
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "color {color}"
            );
        }
    }

    #[test]
    fn accepts_valid_nontransparent_functional_preview_colors() {
        for color in [
            "rgb(255 0 0 / 50%)",
            "rgba(255, 0, 0, 0.5)",
            "hsl(0 100% 50%)",
            "hsl(0 100% 50% / 0.5)",
            "hwb(0 0% 0%)",
            "hwb(0 0% 0% / 50%)",
            "lab(50 20 30)",
            "lab(50 20 30 / 0.5)",
            "lch(50 20 30)",
            "lch(50 20 30 / 50%)",
            "oklab(0.5 0.1 0.1)",
            "oklab(0.5 0.1 0.1 / 0.5)",
            "oklch(0.5 0.1 30)",
            "oklch(0.5 0.1 30 / 50%)",
            "color(display-p3 1 0 0)",
            "color(display-p3 1 0 0 / 0.5)",
        ] {
            let mut value = approved_manifest();
            value["preview"]["accent"] = json!(color);
            assert!(parse(&value).is_ok(), "color {color}");
        }
    }

    #[test]
    fn bounds_and_normalizes_user_visible_text() {
        let mut boundary = approved_manifest();
        boundary["name"] = json!("n".repeat(120));
        boundary["author"] = json!("a".repeat(120));
        boundary["version"] = json!("v".repeat(64));
        assert!(parse(&boundary).is_ok());

        for (field, value) in [
            ("name", String::new()),
            ("name", "n".repeat(121)),
            ("author", String::new()),
            ("author", "a".repeat(121)),
            ("version", String::new()),
            ("version", "v".repeat(65)),
        ] {
            let mut manifest = approved_manifest();
            manifest[field] = json!(value);
            assert_eq!(
                parse(&manifest).unwrap_err().code,
                ThemeErrorCode::InvalidManifest,
                "field {field}"
            );
        }

        let mut decomposed = approved_manifest();
        decomposed["name"] = json!("Cafe\u{301}");
        decomposed["author"] = json!("Ame\u{301}lie");
        decomposed["version"] = json!("be\u{301}ta");
        let parsed = parse(&decomposed).unwrap();
        assert_eq!(parsed.name, "Café");
        assert_eq!(parsed.author.as_deref(), Some("Amélie"));
        assert_eq!(parsed.version.as_deref(), Some("béta"));
    }

    #[test]
    fn rejects_duplicate_unsafe_or_empty_license_file_entries() {
        let invalid_lists = [
            json!([""]),
            json!(["licenses/"]),
            json!(["licenses/LICENSE.rtf"]),
            json!(["assets/LICENSE.txt"]),
            json!(["licenses/../LICENSE.txt"]),
            json!(["licenses//LICENSE.txt"]),
            json!(["/licenses/LICENSE.txt"]),
            json!(["licenses\\LICENSE.txt"]),
            json!(["licenses/Cafe\u{301}.txt", "licenses/Café.txt"]),
        ];

        for license_files in invalid_lists {
            let mut value = approved_manifest();
            value["licenseFiles"] = license_files;
            assert_eq!(
                parse(&value).unwrap_err().code,
                ThemeErrorCode::InvalidManifest
            );
        }

        let mut omitted = approved_manifest();
        omitted.as_object_mut().unwrap().remove("licenseFiles");
        assert!(parse(&omitted).unwrap().license_files.is_empty());
    }
}
