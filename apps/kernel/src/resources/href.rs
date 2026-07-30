use crate::contract::{ResourceName, WorkspaceRelativePath};

use super::{policy::protected_resource_component, ResourceServiceError};

pub fn resolve_markdown_href(
    document_path: &WorkspaceRelativePath,
    href: &str,
) -> Result<WorkspaceRelativePath, ResourceServiceError> {
    if document_path.as_str().is_empty()
        || href.is_empty()
        || href.starts_with(['/', '\\'])
        || href.contains(['\\', '?', '#'])
        || has_uri_scheme(href)
    {
        return Err(ResourceServiceError::invalid_path());
    }

    let mut resolved = document_path
        .as_str()
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _)| {
            parent.split('/').map(str::to_owned).collect()
        });
    for encoded in href.split('/') {
        if encoded.is_empty() {
            return Err(ResourceServiceError::invalid_path());
        }
        let segment = decode_href_segment(encoded)?;
        match segment.as_str() {
            "." => {}
            ".." => {
                if resolved.pop().is_none() {
                    return Err(ResourceServiceError::invalid_path());
                }
            }
            _ => {
                if protected_resource_component(&segment) {
                    return Err(ResourceServiceError::invalid_path());
                }
                let name = ResourceName::parse(segment)
                    .map_err(|_| ResourceServiceError::invalid_path())?;
                resolved.push(name.as_str().to_string());
            }
        }
    }
    if resolved.is_empty() {
        return Err(ResourceServiceError::invalid_path());
    }
    WorkspaceRelativePath::parse(resolved.join("/"))
        .map_err(|_| ResourceServiceError::invalid_path())
}

fn has_uri_scheme(value: &str) -> bool {
    let Some(colon) = value.find(':') else {
        return false;
    };
    let scheme = &value[..colon];
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

fn decode_href_segment(value: &str) -> Result<String, ResourceServiceError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'%' {
            let high = bytes
                .get(cursor + 1)
                .and_then(|byte| hex_value(*byte))
                .ok_or_else(ResourceServiceError::invalid_path)?;
            let low = bytes
                .get(cursor + 2)
                .and_then(|byte| hex_value(*byte))
                .ok_or_else(ResourceServiceError::invalid_path)?;
            decoded.push((high << 4) | low);
            cursor += 3;
        } else {
            decoded.push(bytes[cursor]);
            cursor += 1;
        }
    }
    String::from_utf8(decoded).map_err(|_| ResourceServiceError::invalid_path())
}

const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
