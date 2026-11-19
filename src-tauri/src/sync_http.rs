use reqwest::{redirect::Policy, Client, Method, Url};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Mutex, time::Duration};

const MAX_BYTES: usize = 68 * 1024 * 1024;

#[derive(Clone)]
struct Target {
    bucket: Url,
    prefix: String,
}

impl Target {
    fn new(
        endpoint: &str,
        bucket: &str,
        prefix: &str,
        force_path_style: bool,
    ) -> Result<Self, String> {
        let mut url = Url::parse(endpoint).map_err(|_| "S3 地址格式无效")?;
        let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if !(url.scheme() == "https" || url.scheme() == "http" && local)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("S3 地址需要使用 HTTPS，且不能包含用户信息或查询参数".into());
        }
        if bucket.len() < 3
            || bucket.len() > 63
            || !bucket
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'.')
            || prefix.is_empty()
            || prefix.split('/').any(|p| {
                p.is_empty()
                    || p == "."
                    || p == ".."
                    || !p
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            })
        {
            return Err("Bucket 或同步前缀无效".into());
        }
        if force_path_style {
            url.path_segments_mut()
                .map_err(|_| "S3 地址格式无效")?
                .pop_if_empty()
                .push(bucket);
        } else {
            let host = format!("{bucket}.{}", url.host_str().ok_or("S3 地址格式无效")?);
            url.set_host(Some(&host))
                .map_err(|_| "S3 Bucket 地址无效")?;
        }
        Ok(Self {
            bucket: url,
            prefix: format!("{prefix}/"),
        })
    }

    fn allows(&self, url: &Url, method: &Method) -> bool {
        if url.origin() != self.bucket.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return false;
        }
        let root = self.bucket.path().trim_end_matches('/');
        if url.path().trim_end_matches('/') == root {
            let prefixes: Vec<_> = url
                .query_pairs()
                .filter(|(name, _)| name == "prefix")
                .collect();
            return *method == Method::GET
                && prefixes.len() == 1
                && prefixes[0].1.starts_with(&self.prefix);
        }
        !url.path().contains('%') && url.path().starts_with(&format!("{root}/{}", self.prefix))
    }
}

#[derive(Default)]
pub struct SyncHttp {
    target: Mutex<Option<(Target, Client)>>,
}

#[cfg(target_os = "android")]
struct LocalNetwork(tauri::plugin::PluginHandle<tauri::Wry>);

#[cfg(target_os = "android")]
pub fn local_network_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri::Manager;
    tauri::plugin::Builder::new("local-network")
        .setup(|app, api| {
            app.manage(LocalNetwork(api.register_android_plugin(
                "app.miaoyan.editor.prototype",
                "LocalNetworkPlugin",
            )?));
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn sync_local_network_access(
    window: tauri::WebviewWindow,
    request: bool,
) -> Result<Option<bool>, String> {
    use tauri::Manager;
    if window.label() != "main" {
        return Err("不允许的同步窗口".into());
    }
    window
        .state::<LocalNetwork>()
        .0
        .run_mobile_plugin_async("access", serde_json::json!({ "request": request }))
        .await
        .map_err(|_| "无法读取局域网权限".into())
}

#[tauri::command]
pub fn sync_set_target(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SyncHttp>,
    endpoint: String,
    bucket: String,
    prefix: String,
    force_path_style: Option<bool>,
    timeout: Option<u64>,
    tls_verify: Option<bool>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("不允许的同步窗口".into());
    }
    let target = Target::new(
        &endpoint,
        &bucket,
        &prefix,
        force_path_style.unwrap_or(true),
    )?;
    let client = client(timeout.unwrap_or(45), tls_verify.unwrap_or(true))?;
    *state.target.lock().map_err(|_| "同步网络状态不可用")? = Some((target, client));
    Ok(())
}

fn client(timeout: u64, tls_verify: bool) -> Result<Client, String> {
    if !(1..=300).contains(&timeout) {
        return Err("超时需要为 1–300 秒的整数".into());
    }
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(timeout.min(15)))
        .timeout(Duration::from_secs(timeout))
        .tls_danger_accept_invalid_certs(!tls_verify)
        .build()
        .map_err(|_| "无法初始化同步网络".into())
}

#[derive(Deserialize)]
pub struct Request {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
}

#[derive(Serialize)]
pub struct Response {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Deserialize)]
struct RequestMeta {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    #[serde(rename = "hasBody")]
    has_body: bool,
}

const REQUEST_HEADER: &str = "x-qingyu-request";

// 附件最大 64 MiB，走 JSON 数字数组会膨胀成几百 MB。请求体和响应体都按原始字节传，
// 请求的地址和签名头放在 IPC 头里，响应的状态和头放在字节流开头。
fn decode_request(request: &tauri::ipc::Request<'_>) -> Result<Request, String> {
    use base64::Engine;
    let meta = request
        .headers()
        .get(REQUEST_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or("S3 请求无效")?;
    let meta = base64::engine::general_purpose::STANDARD
        .decode(meta)
        .map_err(|_| "S3 请求无效")?;
    let meta: RequestMeta = serde_json::from_slice(&meta).map_err(|_| "S3 请求无效")?;
    let body = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.clone(),
        // 部分平台的 IPC 回退通道会把字节转成 JSON 数组。
        tauri::ipc::InvokeBody::Json(value) => {
            serde_json::from_value(value.clone()).map_err(|_| "S3 请求内容无效")?
        }
    };
    Ok(Request {
        url: meta.url,
        method: meta.method,
        headers: meta.headers,
        body: meta.has_body.then_some(body),
    })
}

fn encode_response(response: Response) -> Result<Vec<u8>, String> {
    let meta = serde_json::to_vec(&serde_json::json!({
        "status": response.status,
        "headers": response.headers,
    }))
    .map_err(|_| "S3 响应无效")?;
    let mut bytes = Vec::with_capacity(4 + meta.len() + response.body.len());
    bytes.extend_from_slice(&(meta.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&meta);
    bytes.extend_from_slice(&response.body);
    Ok(bytes)
}

#[tauri::command]
pub async fn sync_request(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, SyncHttp>,
    request: tauri::ipc::Request<'_>,
) -> Result<tauri::ipc::Response, String> {
    if window.label() != "main" {
        return Err("不允许的同步窗口".into());
    }
    let request = decode_request(&request)?;
    let (target, client) = state
        .target
        .lock()
        .map_err(|_| "同步网络状态不可用")?
        .clone()
        .ok_or("请先配置同步目标")?;
    let response = perform(&target, &client, request).await?;
    Ok(tauri::ipc::Response::new(encode_response(response)?))
}

async fn perform(target: &Target, client: &Client, request: Request) -> Result<Response, String> {
    let url = Url::parse(&request.url).map_err(|_| "S3 请求地址无效")?;
    let method = Method::from_bytes(request.method.as_bytes()).map_err(|_| "S3 请求方法无效")?;
    if ![Method::GET, Method::HEAD, Method::PUT, Method::DELETE].contains(&method)
        || !target.allows(&url, &method)
    {
        return Err("S3 请求超出当前 Bucket 和同步前缀".into());
    }
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_BYTES)
    {
        return Err("S3 请求内容过大".into());
    }
    let mut outgoing = client.request(method, url);
    for (name, value) in request.headers {
        if ![
            "authorization",
            "content-type",
            "x-amz-content-sha256",
            "x-amz-date",
            "if-match",
            "if-none-match",
            "cache-control",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
        {
            return Err("S3 请求头无效".into());
        }
        outgoing = outgoing.header(name, value);
    }
    if let Some(body) = request.body {
        outgoing = outgoing.body(body);
    }
    let mut response = outgoing.send().await.map_err(network_error)?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return Err("S3 响应内容过大".into());
    }
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "S3 响应读取失败")? {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err("S3 响应内容过大".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn network_error(error: reqwest::Error) -> String {
    use std::error::Error;
    let error = error.without_url();
    let mut message = format!("S3 网络请求失败：{error}");
    let mut cause = error.source();
    while let Some(source) = cause {
        message.push_str(&format!("\n{source}"));
        cause = source.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_transport_options() {
        assert!(client(0, true).is_err());
        assert!(client(301, true).is_err());
        assert!(client(299, true).is_ok());
        assert!(client(45, false).is_ok());
    }
    #[test]
    fn limits_requests_to_the_configured_namespace() {
        let target = Target::new("https://s3.example.test", "test", "miaoyan/v1", true).unwrap();
        let allowed = |url, method| target.allows(&Url::parse(url).unwrap(), &method);
        assert!(allowed(
            "https://s3.example.test/test/miaoyan/v1/objects/abc",
            Method::PUT
        ));
        assert!(allowed(
            "https://s3.example.test/test?list-type=2&prefix=miaoyan%2Fv1%2F",
            Method::GET
        ));
        assert!(!allowed(
            "https://other.example.test/test/miaoyan/v1/a",
            Method::GET
        ));
        assert!(!allowed(
            "https://s3.example.test/test/miaoyan/v10/a",
            Method::DELETE
        ));
        assert!(!allowed(
            "https://s3.example.test/test/other/a",
            Method::DELETE
        ));
        assert!(!allowed(
            "https://s3.example.test/test?prefix=other",
            Method::GET
        ));
        assert!(!allowed(
            "https://s3.example.test/test?prefix=miaoyan%2Fv1%2F&prefix=other",
            Method::GET
        ));
        assert!(!allowed("https://s3.example.test/test", Method::DELETE));
        assert!(!allowed(
            "https://s3.example.test/test/miaoyan/v1/%2e%2e/outside",
            Method::GET
        ));
        assert!(!allowed(
            "https://s3.example.test/test/miaoyan/v1/objects/%2e%2e%2f%2e%2e%2foutside",
            Method::GET
        ));
    }
    #[test]
    fn rejects_unscoped_or_insecure_targets() {
        assert!(Target::new("http://s3.example.test", "test", "miaoyan/v1", true).is_err());
        assert!(Target::new(
            "https://user:pass@s3.example.test",
            "test",
            "miaoyan/v1",
            true
        )
        .is_err());
        assert!(Target::new("https://s3.example.test", "test", "", true).is_err());
        assert!(Target::new("https://s3.example.test", "test", "../outside", true).is_err());
        assert!(Target::new("http://127.0.0.1:9000", "test", "test-prefix", true).is_ok());
    }
    #[test]
    fn frames_response_metadata_ahead_of_raw_body() {
        let bytes = encode_response(Response {
            status: 206,
            headers: HashMap::from([("etag".into(), "\"中\"".into())]),
            body: vec![0, 255, 128],
        })
        .unwrap();
        let length = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize;
        let meta: serde_json::Value = serde_json::from_slice(&bytes[4..4 + length]).unwrap();
        assert_eq!(meta["status"], 206);
        assert_eq!(meta["headers"]["etag"], "\"中\"");
        assert_eq!(&bytes[4 + length..], [0, 255, 128]);
    }
    #[test]
    fn isolates_qingyu_cloud_prefix_for_virtual_hosted_buckets() {
        let target =
            Target::new("https://s3.example.test", "test", "repo/qingyu/v1", false).unwrap();
        for (path, method, expected) in [
            (
                "/?list-type=2&prefix=repo%2Fqingyu%2Fv1%2F",
                Method::GET,
                true,
            ),
            ("/repo/qingyu/v1/objects/test", Method::PUT, true),
            ("/repo/miaoyan/v1/objects/test", Method::PUT, false),
            ("/repo/qingyu/v10/objects/test", Method::DELETE, false),
        ] {
            let url = Url::parse(&format!("https://test.s3.example.test{path}")).unwrap();
            assert_eq!(target.allows(&url, &method), expected);
        }
    }
    #[test]
    fn live_s3_native_transport_when_configured() {
        let Ok(path) = std::env::var("MIAOYAN_S3_PROBE") else {
            return;
        };
        #[derive(Deserialize)]
        struct Probe {
            endpoint: String,
            bucket: String,
            prefix: String,
            cases: Vec<Case>,
        }
        #[derive(Deserialize)]
        struct Case {
            request: Request,
            expected: u16,
        }
        let probe: Probe = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let target = Target::new(&probe.endpoint, &probe.bucket, &probe.prefix, true).unwrap();
        let client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        tauri::async_runtime::block_on(async {
            for case in probe.cases {
                let response = perform(&target, &client, case.request)
                    .await
                    .expect("Native S3 transport failed");
                assert_eq!(
                    response.status, case.expected,
                    "Unexpected native S3 response status"
                );
            }
        });
    }
}
