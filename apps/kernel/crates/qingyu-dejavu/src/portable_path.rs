pub const MAX_PORTABLE_PATH_COMPONENT_BYTES: usize = 255;

pub fn portable_path_component_is_valid(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_PORTABLE_PATH_COMPONENT_BYTES
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }

    !windows_device_stem_is_reserved(value.split('.').next().unwrap_or_default())
}

fn windows_device_stem_is_reserved(stem: &str) -> bool {
    let stem = stem.to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(test)]
mod tests {
    use super::portable_path_component_is_valid;

    #[test]
    fn accepts_names_portable_across_all_supported_platforms() {
        let boundary = "x".repeat(255);
        for valid in [
            "note.md",
            "个人 笔记.md",
            "résumé.md",
            "emoji-📝.md",
            " leading-space.md",
            boundary.as_str(),
        ] {
            assert!(
                portable_path_component_is_valid(valid),
                "rejected {valid:?}"
            );
        }
    }

    #[test]
    fn rejects_cross_platform_forbidden_components() {
        for invalid in [
            "",
            ".",
            "..",
            "bad/name",
            r"bad\name",
            "bad<name",
            "bad>name",
            "bad:name",
            "bad\"name",
            "bad|name",
            "bad?name",
            "bad*name",
            "trailing.",
            "trailing ",
            "control\0name",
            "control\u{001f}name",
            "CON",
            "con.md",
            "PRN.txt",
            "AUX",
            "NUL.md",
            "COM1",
            "com9.md",
            "LPT1",
            "lpt9.txt",
        ] {
            assert!(
                !portable_path_component_is_valid(invalid),
                "accepted {invalid:?}"
            );
        }
        assert!(!portable_path_component_is_valid(&"x".repeat(256)));
    }

    #[test]
    fn accepts_non_reserved_device_like_and_internal_space_components() {
        for valid in [
            "COM0",
            "COM10",
            "LPT0",
            ".qingyu",
            "internal ordinary space.md",
        ] {
            assert!(
                portable_path_component_is_valid(valid),
                "rejected {valid:?}"
            );
        }
    }
}
