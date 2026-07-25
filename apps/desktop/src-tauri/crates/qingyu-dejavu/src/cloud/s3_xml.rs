use futures_util::StreamExt;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Response;
use tokio::io::BufReader;
use tokio_util::io::StreamReader;

use super::s3::validate_relative;
use super::{CloudError, CloudObject};

const MAX_LIST_XML_BYTES: usize = 8 * 1024 * 1024;
const MAX_LIST_FIELD_BYTES: usize = 64 * 1024;
const MAX_PAGE_OBJECTS: usize = 1000;

pub(super) struct ListPage {
    pub objects: Vec<CloudObject>,
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    pub next_continuation_token: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
    Key,
    Size,
    IsTruncated,
    NextContinuationToken,
    Prefix,
}

pub(super) async fn parse_list_page(
    response: Response,
    repository_prefix: &str,
    requested_prefix: &str,
    allow_object_trailing_slash: bool,
) -> Result<ListPage, CloudError> {
    let mut total = 0_usize;
    let stream = response.bytes_stream().map(move |result| {
        let chunk = result.map_err(std::io::Error::other)?;
        total = total
            .checked_add(chunk.len())
            .ok_or_else(|| std::io::Error::other("S3 list XML size overflow"))?;
        if total > MAX_LIST_XML_BYTES {
            return Err(std::io::Error::other("S3 list XML exceeded its bound"));
        }
        Ok(chunk)
    });
    let reader = StreamReader::new(stream);
    let mut xml = Reader::from_reader(BufReader::new(reader));
    xml.config_mut().trim_text(true);

    let repository_prefix = format!("{repository_prefix}/");
    let mut buffer = Vec::new();
    let mut root_open = false;
    let mut root_closed = false;
    let mut depth = 0_usize;
    let mut in_contents = false;
    let mut in_common_prefix = false;
    let mut common_prefix: Option<String> = None;
    let mut current_field = None;
    let mut field_text = String::new();
    let mut object_key: Option<String> = None;
    let mut object_size: Option<u64> = None;
    let mut objects = Vec::new();
    let mut common_prefixes = Vec::new();
    let mut is_truncated = None;
    let mut next_continuation_token = None;

    loop {
        match xml
            .read_event_into_async(&mut buffer)
            .await
            .map_err(|_| CloudError::backend("s3_list_invalid_xml"))?
        {
            Event::Start(start) => {
                let name = start.local_name().as_ref().to_vec();
                if current_field.is_some() || root_closed {
                    return Err(CloudError::backend("s3_list_invalid_xml"));
                }
                match name.as_slice() {
                    b"ListBucketResult" if depth == 0 && !root_open && !root_closed => {
                        root_open = true
                    }
                    b"Contents" if depth == 1 && root_open && !in_contents => {
                        in_contents = true;
                        object_key = None;
                        object_size = None;
                    }
                    b"CommonPrefixes"
                        if depth == 1 && root_open && !in_contents && !in_common_prefix =>
                    {
                        in_common_prefix = true;
                        common_prefix = None;
                    }
                    b"Key" if depth == 2 && in_contents => {
                        start_field(&mut current_field, Field::Key)?;
                        field_text.clear();
                    }
                    b"Size" if depth == 2 && in_contents => {
                        start_field(&mut current_field, Field::Size)?;
                        field_text.clear();
                    }
                    b"IsTruncated" if depth == 1 && root_open && !in_contents => {
                        start_field(&mut current_field, Field::IsTruncated)?;
                        field_text.clear();
                    }
                    b"NextContinuationToken" if depth == 1 && root_open && !in_contents => {
                        start_field(&mut current_field, Field::NextContinuationToken)?;
                        field_text.clear();
                    }
                    b"Prefix" if depth == 2 && in_common_prefix => {
                        start_field(&mut current_field, Field::Prefix)?;
                        field_text.clear();
                    }
                    b"ListBucketResult"
                    | b"Contents"
                    | b"CommonPrefixes"
                    | b"Key"
                    | b"Size"
                    | b"IsTruncated"
                    | b"NextContinuationToken"
                    | b"Prefix" => {
                        return Err(CloudError::backend("s3_list_invalid_xml"));
                    }
                    _ if root_open => {}
                    _ => return Err(CloudError::backend("s3_list_invalid_xml")),
                }
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| CloudError::backend("s3_list_invalid_xml"))?;
            }
            Event::Text(text) => {
                let decoded = text
                    .xml_content()
                    .map_err(|_| CloudError::backend("s3_list_invalid_xml"))?;
                if current_field.is_some() {
                    let decoded = quick_xml::escape::unescape(&decoded)
                        .map_err(|_| CloudError::backend("s3_list_invalid_xml"))?;
                    append_field(&mut field_text, &decoded)?;
                } else if (root_closed || !root_open) && !decoded.trim().is_empty() {
                    return Err(CloudError::backend("s3_list_invalid_xml"));
                }
            }
            Event::CData(text) => {
                if current_field.is_some() {
                    let decoded = text
                        .xml_content()
                        .map_err(|_| CloudError::backend("s3_list_invalid_xml"))?;
                    append_field(&mut field_text, &decoded)?;
                } else if root_closed || !root_open {
                    return Err(CloudError::backend("s3_list_invalid_xml"));
                }
            }
            Event::End(end) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| CloudError::backend("s3_list_invalid_xml"))?;
                let name = end.local_name().as_ref().to_vec();
                match name.as_slice() {
                    b"Key" if depth == 2 && current_field == Some(Field::Key) => {
                        if object_key.replace(field_text.clone()).is_some() {
                            return Err(CloudError::backend("s3_list_duplicate_field"));
                        }
                        current_field = None;
                    }
                    b"Size" if depth == 2 && current_field == Some(Field::Size) => {
                        let size = field_text
                            .parse::<u64>()
                            .map_err(|_| CloudError::backend("s3_list_invalid_size"))?;
                        if object_size.replace(size).is_some() {
                            return Err(CloudError::backend("s3_list_duplicate_field"));
                        }
                        current_field = None;
                    }
                    b"IsTruncated" if depth == 1 && current_field == Some(Field::IsTruncated) => {
                        let value = match field_text.as_str() {
                            "true" => true,
                            "false" => false,
                            _ => return Err(CloudError::backend("s3_list_invalid_truncated")),
                        };
                        if is_truncated.replace(value).is_some() {
                            return Err(CloudError::backend("s3_list_duplicate_field"));
                        }
                        current_field = None;
                    }
                    b"NextContinuationToken"
                        if depth == 1 && current_field == Some(Field::NextContinuationToken) =>
                    {
                        if next_continuation_token
                            .replace(field_text.clone())
                            .is_some()
                        {
                            return Err(CloudError::backend("s3_list_duplicate_field"));
                        }
                        current_field = None;
                    }
                    b"Prefix" if depth == 2 && current_field == Some(Field::Prefix) => {
                        if common_prefix.replace(field_text.clone()).is_some() {
                            return Err(CloudError::backend("s3_list_duplicate_field"));
                        }
                        current_field = None;
                    }
                    b"CommonPrefixes" if depth == 1 && in_common_prefix => {
                        let prefix = common_prefix
                            .take()
                            .ok_or_else(|| CloudError::backend("s3_list_missing_prefix"))?;
                        if prefix.is_empty() || !prefix.starts_with(requested_prefix) {
                            return Err(CloudError::UnsafeKey);
                        }
                        let relative = prefix
                            .strip_prefix(&repository_prefix)
                            .ok_or(CloudError::UnsafeKey)?;
                        validate_relative(relative, true)?;
                        common_prefixes.push(relative.to_string());
                        if objects.len().saturating_add(common_prefixes.len()) > MAX_PAGE_OBJECTS {
                            return Err(CloudError::backend("s3_list_too_many_objects"));
                        }
                        in_common_prefix = false;
                    }
                    b"Contents" if depth == 1 && in_contents => {
                        let key = object_key
                            .take()
                            .ok_or_else(|| CloudError::backend("s3_list_missing_key"))?;
                        let size = object_size
                            .take()
                            .ok_or_else(|| CloudError::backend("s3_list_missing_size"))?;
                        if !key.starts_with(requested_prefix) {
                            return Err(CloudError::UnsafeKey);
                        }
                        let relative = key
                            .strip_prefix(&repository_prefix)
                            .ok_or(CloudError::UnsafeKey)?;
                        validate_relative(relative, allow_object_trailing_slash)?;
                        objects.push(CloudObject {
                            key: relative.to_string(),
                            size,
                        });
                        if objects.len().saturating_add(common_prefixes.len()) > MAX_PAGE_OBJECTS {
                            return Err(CloudError::backend("s3_list_too_many_objects"));
                        }
                        in_contents = false;
                    }
                    b"ListBucketResult"
                        if depth == 0 && root_open && !in_contents && !in_common_prefix =>
                    {
                        root_open = false;
                        root_closed = true;
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) => {}
            Event::Empty(empty)
                if root_open
                    && depth == 1
                    && current_field.is_none()
                    && !is_known_element(empty.local_name().as_ref()) => {}
            Event::Empty(_) | Event::DocType(_) | Event::GeneralRef(_) => {
                return Err(CloudError::backend("s3_list_invalid_xml"));
            }
        }
        buffer.clear();
    }

    if !root_closed
        || root_open
        || depth != 0
        || in_contents
        || in_common_prefix
        || current_field.is_some()
    {
        return Err(CloudError::backend("s3_list_truncated_xml"));
    }
    let is_truncated =
        is_truncated.ok_or_else(|| CloudError::backend("s3_list_missing_truncated"))?;
    if is_truncated && next_continuation_token.as_deref().is_none_or(str::is_empty) {
        return Err(CloudError::backend("s3_list_missing_continuation"));
    }
    Ok(ListPage {
        objects,
        common_prefixes,
        is_truncated,
        next_continuation_token,
    })
}

fn is_known_element(name: &[u8]) -> bool {
    matches!(
        name,
        b"ListBucketResult"
            | b"Contents"
            | b"Key"
            | b"Size"
            | b"IsTruncated"
            | b"NextContinuationToken"
            | b"Prefix"
            | b"CommonPrefixes"
    )
}

fn start_field(current: &mut Option<Field>, field: Field) -> Result<(), CloudError> {
    if current.replace(field).is_some() {
        return Err(CloudError::backend("s3_list_nested_field"));
    }
    Ok(())
}

fn append_field(output: &mut String, value: &str) -> Result<(), CloudError> {
    let length = output
        .len()
        .checked_add(value.len())
        .ok_or_else(|| CloudError::backend("s3_list_field_too_large"))?;
    if length > MAX_LIST_FIELD_BYTES {
        return Err(CloudError::backend("s3_list_field_too_large"));
    }
    output.push_str(value);
    Ok(())
}
