use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::{
    dns::{Addrs, Name, Resolve, Resolving},
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
    Client, Url,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::IpAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::Semaphore;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    target: String,
    image: bool,
}

#[derive(Serialize)]
pub(crate) struct PreviewResource {
    url: String,
    mime: String,
    content: String,
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !matches!(
            ip.octets(),
            [0 | 10 | 127 | 224..=255, ..]
                | [100, 64..=127, ..]
                | [169, 254, ..]
                | [172, 16..=31, ..]
                | [192, 0, 0 | 2, _]
                | [192, 88, 99, _]
                | [192, 168, ..]
                | [198, 18 | 19, ..]
                | [198, 51, 100, _]
                | [203, 0, 113, _]
        ),
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            (0x2000..=0x3fff).contains(&a)
                && !matches!(
                    (a, b),
                    (0x2001, 0..=0x1ff | 0xdb8) | (0x2002, _) | (0x3fff, _)
                )
        }
    }
}

fn public_target(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if let Ok(ip) = host.trim_matches(['[', ']']).parse() {
        return public_ip(ip);
    }
    let host = host.trim_end_matches('.');
    host.contains('.') && !host.ends_with(".local") && !host.ends_with(".localhost")
}

struct PublicDns;

impl Resolve for PublicDns {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            // Resolve at connect time, so every address the connector can dial is checked.
            let addrs: Vec<_> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|addr| public_ip(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Private preview address",
                )
                .into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
static REQUESTS: Semaphore = Semaphore::const_new(8);

pub(crate) async fn fetch(request: PreviewRequest) -> Option<PreviewResource> {
    if request.target.len() > 4096 {
        return None;
    }
    let url = Url::parse(&request.target).ok()?;
    if !public_target(&url) {
        return None;
    }
    let _permit = REQUESTS.try_acquire().ok()?;
    let client = CLIENT
        .get_or_init(|| {
            Client::builder()
                .no_proxy()
                .referer(false)
                .dns_resolver(Arc::new(PublicDns))
                .redirect(Policy::custom(|attempt| {
                    if attempt.previous().len() < 5 && public_target(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.stop()
                    }
                }))
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(8))
                .user_agent("QingYu link preview")
                .build()
                .ok()
        })
        .as_ref()?;
    let accept = if request.image {
        "image/avif,image/webp,image/png,image/jpeg,image/gif"
    } else {
        "text/html,application/xhtml+xml"
    };
    let mut response = client
        .get(url)
        .header(ACCEPT, accept)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let url = response.url().as_str().to_owned();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)?
        .to_str()
        .ok()?
        .to_owned();
    let mime = content_type.split(';').next()?.trim().to_ascii_lowercase();
    let limit = if request.image {
        if !matches!(
            mime.as_str(),
            "image/avif"
                | "image/webp"
                | "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/x-icon"
                | "image/vnd.microsoft.icon"
        ) {
            return None;
        }
        768 * 1024
    } else {
        if !matches!(mime.as_str(), "text/html" | "application/xhtml+xml") {
            return None;
        }
        300 * 1024
    };
    if request.image
        && response
            .content_length()
            .is_some_and(|size| size > limit as u64)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        let remaining = limit - bytes.len();
        if request.image && chunk.len() > remaining {
            return None;
        }
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        if !request.image && bytes.len() == limit {
            break;
        }
    }
    if bytes.is_empty() {
        return None;
    }
    let content = if request.image {
        STANDARD.encode(bytes)
    } else {
        let charset = content_type.split(';').skip(1).find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            key.eq_ignore_ascii_case("charset")
                .then(|| value.trim().trim_matches('"'))
        });
        let encoding = charset
            .and_then(|name| encoding_rs::Encoding::for_label(name.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        encoding.decode(&bytes).0.into_owned()
    };
    Some(PreviewResource { url, mime, content })
}

#[cfg(feature = "app")]
#[tauri::command]
pub(crate) async fn fetch_link_preview(request: PreviewRequest) -> Option<PreviewResource> {
    fetch(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_dns_results_never_reach_the_connector() {
        let name = "localhost".parse::<Name>().unwrap();
        assert!(PublicDns.resolve(name).await.is_err());
    }

    #[test]
    fn previews_only_dial_ordinary_http_ports_and_public_addresses() {
        for target in [
            "http://127.1/",
            "http://0x7f000001/",
            "http://169.254.169.254/",
            "http://100.64.0.1/",
            "http://[::ffff:127.0.0.1]/",
            "http://[2002:7f00:1::]/",
            "http://[64:ff9b::7f00:1]/",
            "http://[fd00::1]/",
            "http://localhost./",
            "http://router.local./",
            "https://example.com:444/",
            "https://secret@example.com/",
            "file:///etc/passwd",
        ] {
            assert!(!public_target(&Url::parse(target).unwrap()), "{target}");
        }
        for target in [
            "https://example.com/",
            "https://1.1.1.1:443/",
            "http://[2606:4700:4700::1111]/",
        ] {
            assert!(public_target(&Url::parse(target).unwrap()), "{target}");
        }
    }
}
