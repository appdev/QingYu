use std::collections::{BTreeMap, BTreeSet};

use cssparser::{BasicParseErrorKind, ParseError, ParseErrorKind, Parser, ParserInput, Token};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::{
    manifest::{
        normalize_bounded_text, normalize_preview_color, parse_theme_appearance, valid_theme_id,
    },
    ParsedTheme, ThemeDescriptor, ThemeError, ThemeErrorCode, ThemePreview, ThemeStorageKind,
};

pub(crate) const MAX_THEME_BYTES: usize = 256 * 1024;

const REQUIRED_METADATA: [&str; 7] = [
    "id",
    "name",
    "appearance",
    "preview-background",
    "preview-panel",
    "preview-text",
    "preview-accent",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidatedPackageCss {
    pub(crate) css: String,
    pub(crate) referenced_assets: BTreeSet<String>,
}

pub(crate) fn parse_theme_file(bytes: &[u8], file_name: &str) -> Result<ParsedTheme, ThemeError> {
    if bytes.len() > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorCode::ThemeTooLarge,
            "Theme files cannot exceed 256 KiB.",
        ));
    }

    let css = std::str::from_utf8(bytes).map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::InvalidUtf8,
            "Theme files must use UTF-8 encoding.",
        )
    })?;
    let metadata = parse_metadata(css)?;
    validate_css(
        css,
        "Theme CSS cannot import or load external or relative resources.",
        validate_legacy_resource_url,
    )?;

    let appearance = metadata
        .get("appearance")
        .and_then(|value| parse_theme_appearance(value))
        .ok_or_else(|| {
            ThemeError::new(
                ThemeErrorCode::InvalidMetadata,
                "Theme appearance must be light or dark.",
            )
        })?;
    let id = required(&metadata, "id")?;
    if !valid_theme_id(id) {
        return Err(ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            "Theme ID is invalid or reserved.",
        ));
    }

    let name = bounded_text(required(&metadata, "name")?, 120, "name")?;
    let author = optional_bounded_text(&metadata, "author", 120)?;
    let version = optional_bounded_text(&metadata, "version", 64)?;
    let preview = ThemePreview {
        accent: preview_color(&metadata, "preview-accent")?,
        background: preview_color(&metadata, "preview-background")?,
        panel: preview_color(&metadata, "preview-panel")?,
        text: preview_color(&metadata, "preview-text")?,
    };
    let fingerprint = format!("{:x}", Sha256::digest(bytes));

    Ok(ParsedTheme {
        bytes: bytes.to_vec(),
        descriptor: ThemeDescriptor {
            appearance,
            author,
            file_name: file_name.to_string(),
            fingerprint,
            id: id.to_string(),
            name,
            preview,
            source: "third-party".to_string(),
            storage_kind: ThemeStorageKind::InlineCss,
            version,
        },
    })
}

pub(crate) fn validate_package_css(bytes: &[u8]) -> Result<ValidatedPackageCss, ThemeError> {
    if bytes.len() > MAX_THEME_BYTES {
        return Err(ThemeError::new(
            ThemeErrorCode::ThemeTooLarge,
            "Theme CSS cannot exceed 256 KiB.",
        ));
    }
    let css = std::str::from_utf8(bytes).map_err(|_| {
        ThemeError::new(
            ThemeErrorCode::InvalidUtf8,
            "Theme CSS must use UTF-8 encoding.",
        )
    })?;
    let mut referenced_assets = BTreeSet::new();
    validate_css(
        css,
        "Theme CSS contains an unsafe package resource reference.",
        |value| validate_package_resource_url(value, &mut referenced_assets),
    )?;

    Ok(ValidatedPackageCss {
        css: css.to_string(),
        referenced_assets,
    })
}

pub(crate) fn validate_svg_css(css: &str) -> Result<(), ThemeError> {
    validate_css(
        css,
        "Theme SVG contains an unsafe CSS resource reference.",
        validate_legacy_resource_url,
    )
}

fn parse_metadata(css: &str) -> Result<BTreeMap<String, String>, ThemeError> {
    let css = css.trim_start_matches('\u{feff}').trim_start();
    let rest = css.strip_prefix("/*").ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            "The first theme block must be a @qingyu-theme comment.",
        )
    })?;
    let end = rest.find("*/").ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            "The theme metadata comment is not closed.",
        )
    })?;
    let mut lines = rest[..end]
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if lines.next() != Some("@qingyu-theme") {
        return Err(ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            "The first theme block must begin with @qingyu-theme.",
        ));
    }

    let mut metadata = BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once(':').ok_or_else(|| {
            ThemeError::new(
                ThemeErrorCode::InvalidMetadata,
                "Every metadata line must use key: value syntax.",
            )
        })?;
        let key = key.trim();
        let value = value.trim();
        if !valid_metadata_key(key) || value.is_empty() || value.contains(['\r', '\n']) {
            return Err(ThemeError::new(
                ThemeErrorCode::InvalidMetadata,
                "Theme metadata contains an invalid key or value.",
            ));
        }
        if value.chars().count() > 256
            || metadata
                .insert(key.to_string(), value.to_string())
                .is_some()
        {
            return Err(ThemeError::new(
                ThemeErrorCode::InvalidMetadata,
                "Theme metadata keys must be unique and values must be bounded.",
            ));
        }
    }

    if REQUIRED_METADATA
        .iter()
        .any(|key| !metadata.contains_key(*key))
    {
        return Err(ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            "Theme metadata is missing a required field.",
        ));
    }

    Ok(metadata)
}

fn valid_metadata_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn required<'a>(metadata: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, ThemeError> {
    metadata.get(key).map(String::as_str).ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            format!("Theme metadata is missing {key}."),
        )
    })
}

fn bounded_text(value: &str, max: usize, field: &str) -> Result<String, ThemeError> {
    normalize_bounded_text(value, max).ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            format!("Theme {field} is empty or too long."),
        )
    })
}

fn optional_bounded_text(
    metadata: &BTreeMap<String, String>,
    key: &str,
    max: usize,
) -> Result<Option<String>, ThemeError> {
    metadata
        .get(key)
        .map(|value| bounded_text(value, max, key))
        .transpose()
}

fn preview_color(metadata: &BTreeMap<String, String>, key: &str) -> Result<String, ThemeError> {
    normalize_preview_color(required(metadata, key)?).ok_or_else(|| {
        ThemeError::new(
            ThemeErrorCode::InvalidMetadata,
            format!("Theme {key} must be a non-transparent CSS color."),
        )
    })
}

fn validate_css<F>(
    css: &str,
    unsafe_resource_message: &'static str,
    mut url_policy: F,
) -> Result<(), ThemeError>
where
    F: FnMut(&str) -> Result<(), ThemeErrorCode>,
{
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    validate_tokens(&mut parser, &mut url_policy).map_err(|code| {
        let message = match code {
            ThemeErrorCode::UnsafeResource => unsafe_resource_message,
            _ => "Theme CSS contains invalid syntax.",
        };
        ThemeError::new(code, message)
    })
}

fn validate_tokens<'i, 't>(
    parser: &mut Parser<'i, 't>,
    url_policy: &mut dyn FnMut(&str) -> Result<(), ThemeErrorCode>,
) -> Result<(), ThemeErrorCode> {
    validate_tokens_with_context(parser, url_policy, false)
}

fn validate_tokens_with_context<'i, 't>(
    parser: &mut Parser<'i, 't>,
    url_policy: &mut dyn FnMut(&str) -> Result<(), ThemeErrorCode>,
    quoted_strings_are_urls: bool,
) -> Result<(), ThemeErrorCode> {
    loop {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(error) if matches!(error.kind, BasicParseErrorKind::EndOfInput) => return Ok(()),
            Err(_) => return Err(ThemeErrorCode::InvalidCss),
        };

        if token.is_parse_error() {
            return Err(ThemeErrorCode::InvalidCss);
        }

        match token {
            Token::AtKeyword(name) if name.eq_ignore_ascii_case("import") => {
                return Err(ThemeErrorCode::UnsafeResource)
            }
            Token::QuotedString(value) if quoted_strings_are_urls => url_policy(&value)?,
            Token::UnquotedUrl(value) => url_policy(&value)?,
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
                parser
                    .parse_nested_block(|nested| parse_url_function(nested, url_policy))
                    .map_err(theme_error_code_from_parse_error)?;
            }
            Token::Function(name) if is_image_set_function(&name) => {
                parser
                    .parse_nested_block(|nested| {
                        validate_tokens_with_context(nested, url_policy, true)
                            .map_err(|code| nested.new_custom_error(code))
                    })
                    .map_err(theme_error_code_from_parse_error)?;
            }
            Token::Function(name)
                if quoted_strings_are_urls && is_deferred_value_function(&name) =>
            {
                return Err(ThemeErrorCode::UnsafeResource);
            }
            Token::Function(_) => {
                parser
                    .parse_nested_block(|nested| {
                        validate_tokens_with_context(nested, url_policy, false)
                            .map_err(|code| nested.new_custom_error(code))
                    })
                    .map_err(theme_error_code_from_parse_error)?;
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                parser
                    .parse_nested_block(|nested| {
                        validate_tokens_with_context(nested, url_policy, quoted_strings_are_urls)
                            .map_err(|code| nested.new_custom_error(code))
                    })
                    .map_err(theme_error_code_from_parse_error)?;
            }
            _ => {}
        }
    }
}

fn is_image_set_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("image-set") || name.eq_ignore_ascii_case("-webkit-image-set")
}

fn is_deferred_value_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("var")
        || name.eq_ignore_ascii_case("env")
        || name.eq_ignore_ascii_case("attr")
}

fn theme_error_code_from_parse_error(error: ParseError<'_, ThemeErrorCode>) -> ThemeErrorCode {
    match error.kind {
        ParseErrorKind::Custom(code) => code,
        ParseErrorKind::Basic(_) => ThemeErrorCode::InvalidCss,
    }
}

fn parse_url_function<'i, 't>(
    parser: &mut Parser<'i, 't>,
    url_policy: &mut dyn FnMut(&str) -> Result<(), ThemeErrorCode>,
) -> Result<(), ParseError<'i, ThemeErrorCode>> {
    let token = parser
        .next()
        .map_err(ParseError::<ThemeErrorCode>::from)?
        .clone();
    let value = match token {
        Token::QuotedString(value) | Token::UnquotedUrl(value) => value,
        _ => return Err(parser.new_custom_error(ThemeErrorCode::UnsafeResource)),
    };
    url_policy(&value).map_err(|code| parser.new_custom_error(code))?;
    parser
        .expect_exhausted()
        .map_err(ParseError::<ThemeErrorCode>::from)
}

fn validate_legacy_resource_url(value: &str) -> Result<(), ThemeErrorCode> {
    let value = value.trim();
    if value.starts_with('#') || value.to_ascii_lowercase().starts_with("data:") {
        return Ok(());
    }
    Err(ThemeErrorCode::UnsafeResource)
}

fn validate_package_resource_url(
    value: &str,
    referenced_assets: &mut BTreeSet<String>,
) -> Result<(), ThemeErrorCode> {
    let value = value.trim();
    if value.starts_with('#') || value.to_ascii_lowercase().starts_with("data:") {
        return Ok(());
    }
    if has_malformed_percent_encoding(value) {
        return Err(ThemeErrorCode::UnsafeResource);
    }
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| ThemeErrorCode::UnsafeResource)?;
    let normalized: String = decoded.nfc().collect::<String>().replace('\\', "/");
    if normalized.contains(['\0', '?', '#']) {
        return Err(ThemeErrorCode::UnsafeResource);
    }
    let path = normalized
        .strip_prefix("./assets/")
        .ok_or(ThemeErrorCode::UnsafeResource)?;
    let segments: Vec<&str> = path.split('/').collect();
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.is_empty() || matches!(*segment, "." | ".."))
    {
        return Err(ThemeErrorCode::UnsafeResource);
    }
    referenced_assets.insert(format!("assets/{path}"));
    Ok(())
}

fn has_malformed_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return true;
        }
        index += 3;
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{parse_theme_file, validate_package_css};
    use crate::themes::{ThemeAppearance, ThemeErrorCode};

    fn theme_css(id: &str, name: &str, appearance: &str, body: &str) -> Vec<u8> {
        format!(
            "/*\n@qingyu-theme\nid: {id}\nname: {name}\nappearance: {appearance}\npreview-background: #2e3440\npreview-panel: #3b4252\npreview-text: #eceff4\npreview-accent: #88c0d0\n*/\n{body}\n"
        )
        .into_bytes()
    }

    #[test]
    fn parses_unicode_metadata_and_safe_urls() {
        let parsed = parse_theme_file(
            &theme_css(
                "nord",
                "北境",
                "dark",
                ":root { background: url(data:image/svg+xml;base64,AA==); mask: url(#marker); }",
            ),
            "nord.css",
        )
        .unwrap();

        assert_eq!(parsed.descriptor.id, "nord");
        assert_eq!(parsed.descriptor.name, "北境");
        assert_eq!(parsed.descriptor.appearance, ThemeAppearance::Dark);
        assert_eq!(parsed.descriptor.file_name, "nord.css");
        assert_eq!(parsed.descriptor.fingerprint.len(), 64);
    }

    #[test]
    fn legacy_css_still_requires_metadata_and_rejects_all_relative_urls() {
        assert_eq!(
            parse_theme_file(b":root { color: red; }", "plain.css")
                .unwrap_err()
                .code,
            ThemeErrorCode::InvalidMetadata
        );

        for url in [
            "asset.png",
            "./asset.png",
            "../asset.png",
            "./assets/fonts/JetBrainsMono-Regular.woff2",
        ] {
            let body = format!(":root {{ background: url(\"{url}\"); }}");
            assert_eq!(
                parse_theme_file(
                    &theme_css("legacy-relative", "Legacy", "light", &body),
                    "legacy-relative.css",
                )
                .unwrap_err()
                .code,
                ThemeErrorCode::UnsafeResource,
                "url {url}"
            );
        }
    }

    #[test]
    fn package_css_accepts_assets_fragments_and_bounded_data_urls() {
        let css = validate_package_css(
            ":root {
                src: url(\"./assets/fonts/JetBrainsMono-Regular.woff2\");
                mask: url(#marker);
                background: url(data:image/svg+xml;base64,AA==);
                background-image: image-set(\"./assets/images/background.png\" type(\"image/png\") 1x);
                content: -webkit-image-set(\"./assets/images/background@2x.png\" 2x);
                cursor: url(\"./assets/icons/Cafe%CC%81.svg\"), auto;
            }"
            .as_bytes(),
        )
        .unwrap();

        assert_eq!(
            css.referenced_assets,
            BTreeSet::from([
                "assets/fonts/JetBrainsMono-Regular.woff2".to_string(),
                "assets/icons/Café.svg".to_string(),
                "assets/images/background.png".to_string(),
                "assets/images/background@2x.png".to_string(),
            ])
        );
        assert!(css.css.contains("JetBrainsMono-Regular.woff2"));
    }

    #[test]
    fn package_css_rejects_imports_and_unsafe_resource_urls() {
        for body in [
            "@import './assets/theme.css';",
            ":root { src: url(https://example.com/font.woff2); }",
            ":root { src: url(//example.com/font.woff2); }",
            ":root { src: url(file:///tmp/font.woff2); }",
            ":root { src: url(/assets/font.woff2); }",
            ":root { src: url(font.woff2); }",
            ":root { src: url(../font.woff2); }",
            ":root { src: url(./assets/%2e%2e/licenses/x.txt); }",
            ":root { src: url(./assets/font.woff2?cache=1); }",
            ":root { src: url(./assets/font.woff2#face); }",
            ":root { src: url(./assets/../licenses/x.txt); }",
            ":root { background: image-set(\"https://example.com/pixel.png\" 1x); }",
            ":root { background: -webkit-image-set(\"//example.com/pixel.png\" 2x); }",
            ":root { --pixel: \"https://example.com/pixel.png\"; background: image-set(var(--pixel) 1x); }",
        ] {
            assert_eq!(
                validate_package_css(body.as_bytes()).unwrap_err().code,
                ThemeErrorCode::UnsafeResource,
                "body {body}"
            );
        }
    }

    #[test]
    fn package_css_bounds_data_urls_with_the_css_limit() {
        let mut css = b":root { background: url(data:image/png;base64,".to_vec();
        css.resize(super::MAX_THEME_BYTES + 1, b'A');
        assert_eq!(
            validate_package_css(&css).unwrap_err().code,
            ThemeErrorCode::ThemeTooLarge
        );
    }

    #[test]
    fn rejects_import_and_unsafe_resource_urls() {
        for body in [
            "@import 'other.css';",
            ":root { background: url(https://example.com/a.png); }",
            ":root { background: url(//example.com/a.png); }",
            ":root { background: url(file:///tmp/a.png); }",
            ":root { background: url(asset.png); }",
            ":root { background: url(../asset.png); }",
            ":root { background: image-set(\"https://example.com/a.png\" 1x); }",
            ":root { background: -webkit-image-set(\"asset.png\" 2x); }",
        ] {
            let error =
                parse_theme_file(&theme_css("unsafe", "Unsafe", "light", body), "unsafe.css")
                    .unwrap_err();
            assert!(matches!(
                error.code,
                ThemeErrorCode::UnsafeResource | ThemeErrorCode::InvalidCss
            ));
        }
    }

    #[test]
    fn rejects_reserved_or_malformed_ids() {
        for id in ["light", "dark", "qingyu-owned", "Upper", "-leading"] {
            let error = parse_theme_file(&theme_css(id, "Bad", "light", ":root {}"), "bad.css")
                .unwrap_err();
            assert_eq!(error.code, ThemeErrorCode::InvalidMetadata);
        }
    }

    #[test]
    fn rejects_duplicate_missing_or_transparent_preview_metadata() {
        let duplicate = String::from_utf8(theme_css("dup", "Dup", "light", ":root {}"))
            .unwrap()
            .replace("name: Dup", "name: Dup\nname: Again");
        assert_eq!(
            parse_theme_file(duplicate.as_bytes(), "dup.css")
                .unwrap_err()
                .code,
            ThemeErrorCode::InvalidMetadata
        );

        let missing = String::from_utf8(theme_css("missing", "Missing", "light", ":root {}"))
            .unwrap()
            .replace("preview-accent: #88c0d0\n", "");
        assert_eq!(
            parse_theme_file(missing.as_bytes(), "missing.css")
                .unwrap_err()
                .code,
            ThemeErrorCode::InvalidMetadata
        );

        let transparent =
            String::from_utf8(theme_css("transparent", "Transparent", "light", ":root {}"))
                .unwrap()
                .replace("preview-accent: #88c0d0", "preview-accent: #00000000");
        assert_eq!(
            parse_theme_file(transparent.as_bytes(), "transparent.css")
                .unwrap_err()
                .code,
            ThemeErrorCode::InvalidMetadata
        );
    }

    #[test]
    fn rejects_invalid_utf8_oversized_and_broken_css() {
        assert_eq!(
            parse_theme_file(&[0xff, 0xfe], "bad.css").unwrap_err().code,
            ThemeErrorCode::InvalidUtf8
        );
        assert_eq!(
            parse_theme_file(&vec![b'a'; 256 * 1024 + 1], "large.css")
                .unwrap_err()
                .code,
            ThemeErrorCode::ThemeTooLarge
        );
        assert_eq!(
            parse_theme_file(
                &theme_css(
                    "broken",
                    "Broken",
                    "light",
                    ":root { color: \"unterminated; }"
                ),
                "broken.css",
            )
            .unwrap_err()
            .code,
            ThemeErrorCode::InvalidCss
        );
    }
}
