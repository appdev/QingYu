#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppLanguage {
    En,
    ZhCn,
    ZhTw,
    Ja,
    Ko,
    Fr,
    De,
    Es,
    PtBr,
    It,
    Ru,
}

impl AppLanguage {
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Es => "es",
            Self::PtBr => "pt-BR",
            Self::It => "it",
            Self::Ru => "ru",
        }
    }

    pub(crate) fn from_code(value: &str) -> Option<Self> {
        match value {
            "en" => Some(Self::En),
            "zh-CN" => Some(Self::ZhCn),
            "zh-TW" => Some(Self::ZhTw),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "fr" => Some(Self::Fr),
            "de" => Some(Self::De),
            "es" => Some(Self::Es),
            "pt-BR" => Some(Self::PtBr),
            "it" => Some(Self::It),
            "ru" => Some(Self::Ru),
            _ => None,
        }
    }
}

pub(crate) fn resolve_startup_language(_identifier: &str) -> AppLanguage {
    let system_locales = system_locale_candidates();
    let system_locale_refs = system_locales
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    language_for_initial_launch(&system_locale_refs)
}

pub(crate) fn language_for_initial_launch(system_locales: &[&str]) -> AppLanguage {
    language_from_system_locales(system_locales).unwrap_or(AppLanguage::En)
}

fn system_locale_candidates() -> Vec<String> {
    sys_locale::get_locales().collect()
}

fn language_from_system_locales(locales: &[&str]) -> Option<AppLanguage> {
    locales
        .iter()
        .find_map(|locale| language_from_locale(locale))
}

fn language_from_locale(locale: &str) -> Option<AppLanguage> {
    let normalized = normalize_locale(locale);

    if normalized.is_empty() {
        return None;
    }

    if normalized == "zh" || normalized.starts_with("zh-") {
        return if normalized.contains("hant")
            || normalized.starts_with("zh-tw")
            || normalized.starts_with("zh-hk")
            || normalized.starts_with("zh-mo")
        {
            Some(AppLanguage::ZhTw)
        } else {
            Some(AppLanguage::ZhCn)
        };
    }

    if normalized == "pt" || normalized.starts_with("pt-") {
        return Some(AppLanguage::PtBr);
    }

    let language = normalized.split('-').next().unwrap_or_default();

    match language {
        "en" => Some(AppLanguage::En),
        "ja" => Some(AppLanguage::Ja),
        "ko" => Some(AppLanguage::Ko),
        "fr" => Some(AppLanguage::Fr),
        "de" => Some(AppLanguage::De),
        "es" => Some(AppLanguage::Es),
        "it" => Some(AppLanguage::It),
        "ru" => Some(AppLanguage::Ru),
        _ => None,
    }
}

fn normalize_locale(locale: &str) -> String {
    locale
        .trim()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn native_startup_uses_the_supported_system_language() {
        let language = language_for_initial_launch(&["zh_CN"]);

        assert_eq!(language, AppLanguage::ZhCn);
    }

    #[test]
    fn missing_language_uses_supported_system_locale() {
        let language = language_for_initial_launch(&["zh_Hant_TW"]);

        assert_eq!(language, AppLanguage::ZhTw);
    }

    #[test]
    fn unsupported_system_locale_defaults_to_english() {
        let language = language_for_initial_launch(&["nl_NL"]);

        assert_eq!(language, AppLanguage::En);
    }

    #[test]
    fn locale_matching_uses_the_first_supported_system_language() {
        let language = language_for_initial_launch(&["nl_NL", "ja_JP"]);

        assert_eq!(language, AppLanguage::Ja);
    }

    #[test]
    fn macos_bundle_declares_all_supported_localizations() {
        let expected_localizations = [
            (AppLanguage::En, "en"),
            (AppLanguage::ZhCn, "zh-Hans"),
            (AppLanguage::ZhTw, "zh-Hant"),
            (AppLanguage::Ja, "ja"),
            (AppLanguage::Ko, "ko"),
            (AppLanguage::Fr, "fr"),
            (AppLanguage::De, "de"),
            (AppLanguage::Es, "es"),
            (AppLanguage::PtBr, "pt-BR"),
            (AppLanguage::It, "it"),
            (AppLanguage::Ru, "ru"),
        ];
        let info_plist = include_str!("../Info.plist");
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("Tauri config should be valid JSON");
        let bundled_files = config
            .pointer("/bundle/macOS/files")
            .and_then(serde_json::Value::as_object)
            .expect("macOS bundle config should include localized resource files");
        let resources_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("macos-locales");

        for (language, macos_code) in expected_localizations {
            assert!(
                info_plist.contains(&format!("<string>{macos_code}</string>")),
                "Info.plist should declare {} as {macos_code}",
                language.as_code()
            );

            let source = format!("macos-locales/{macos_code}.lproj");
            let destination = format!("Resources/{macos_code}.lproj");
            assert_eq!(
                bundled_files
                    .get(&destination)
                    .and_then(serde_json::Value::as_str),
                Some(source.as_str()),
                "missing bundle resource mapping for {}",
                language.as_code()
            );

            let strings_path = resources_root
                .join(format!("{macos_code}.lproj"))
                .join("InfoPlist.strings");
            let strings = fs::read_to_string(&strings_path)
                .unwrap_or_else(|error| panic!("{}: {error}", strings_path.display()));
            assert!(
                strings.contains("CFBundleDisplayName"),
                "{} should localize the bundle display name",
                strings_path.display()
            );
        }
    }

    #[test]
    fn macos_bundle_declares_local_network_usage() {
        let info_plist = include_str!("../Info.plist");

        assert!(info_plist.contains("<key>NSLocalNetworkUsageDescription</key>"));
        assert!(info_plist
            .contains("QingYu uses your local network to connect to sync servers you configure."));
    }

    #[test]
    fn generated_apple_project_versions_match_the_crate_version() {
        let version = env!("CARGO_PKG_VERSION");
        let tauri_config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json"))
                .expect("Tauri config should be valid JSON");
        let apple_project = include_str!("../gen/apple/project.yml");
        let ios_info_plist = include_str!("../gen/apple/qingyu_iOS/Info.plist");

        assert_eq!(
            tauri_config
                .get("version")
                .and_then(serde_json::Value::as_str),
            Some(version)
        );
        assert!(apple_project.contains(&format!("CFBundleShortVersionString: {version}")));
        assert!(apple_project.contains(&format!("CFBundleVersion: \"{version}\"")));
        assert!(ios_info_plist.contains(&format!(
            "<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>"
        )));
        assert!(ios_info_plist.contains(&format!(
            "<key>CFBundleVersion</key>\n\t<string>{version}</string>"
        )));
    }
}
