use reqwest::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncValueIssue {
    InvalidPath,
    InvalidUrl,
    OutOfRange,
    Required,
}

pub(crate) fn normalize_interval(value: u32) -> u32 {
    value.min(1_440)
}

pub(crate) fn normalize_non_secret(value: &mut String) {
    value.truncate(value.trim_end().len());
    let trimmed_start = value.len() - value.trim_start().len();
    value.drain(..trimmed_start);
}

pub(crate) fn validate_remote_root(value: &str) -> Option<SyncValueIssue> {
    normalize_remote_root(value).err()
}

pub(crate) fn normalize_remote_root(value: &str) -> Result<String, SyncValueIssue> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(SyncValueIssue::Required);
    }
    if trimmed.contains('\0')
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed
            .as_bytes()
            .get(0..2)
            .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':')
    {
        return Err(SyncValueIssue::InvalidPath);
    }
    let normalized = trimmed.replace('\\', "/");
    if normalized
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(SyncValueIssue::InvalidPath);
    }
    Ok(normalized)
}

pub(crate) fn validate_http_url(value: &str) -> Option<SyncValueIssue> {
    if value.trim().is_empty() {
        return Some(SyncValueIssue::Required);
    }
    if Url::parse(value.trim())
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        None
    } else {
        Some(SyncValueIssue::InvalidUrl)
    }
}

pub(crate) fn validate_required(value: &str) -> Option<SyncValueIssue> {
    value.trim().is_empty().then_some(SyncValueIssue::Required)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_root_rejects_absolute_drive_unc_and_ambiguous_segments_cross_platform() {
        for value in [
            "/notes",
            r"\notes",
            r"\\server\share",
            r"C:\notes",
            "C:/notes",
            "notes/",
            r"notes\",
            "notes//images",
            r"notes\\images",
            ".",
            "notes/./images",
            "notes/../private",
            "notes\0private",
        ] {
            assert_eq!(
                validate_remote_root(value),
                Some(SyncValueIssue::InvalidPath),
                "{value:?} must be rejected"
            );
        }
    }

    #[test]
    fn remote_root_normalizes_only_a_valid_relative_path() {
        assert_eq!(
            normalize_remote_root(" team\\notes ").unwrap(),
            "team/notes"
        );
        assert_eq!(normalize_remote_root("team/notes").unwrap(), "team/notes");
        assert_eq!(
            normalize_remote_root("../private"),
            Err(SyncValueIssue::InvalidPath)
        );
    }

    #[test]
    fn empty_remote_root_remains_a_required_issue() {
        assert_eq!(validate_remote_root("  "), Some(SyncValueIssue::Required));
    }
}
