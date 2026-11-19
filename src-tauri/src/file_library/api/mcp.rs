use super::*;
use axum::{body::to_bytes, extract::Extension, routing::post};
use serde_json::Value;

const PROTOCOL: &str = "2025-11-25";
const SUPPORTED: [&str; 2] = [PROTOCOL, "2025-06-18"];

#[derive(Clone, Copy)]
struct CanWrite(bool);

pub(super) fn router(state: Arc<Service>) -> Router<Arc<Service>> {
    Router::new()
        .route("/mcp", post(exchange).fallback(no_stream))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(state, authenticate))
}

async fn authenticate(
    State(state): State<Arc<Service>>,
    mut request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let (write, read) = bearer_access(&state, headers);
    // Native clients connect directly; browser Origins are not accepted, even after DNS rebinding.
    let mut response = if headers.contains_key(header::ORIGIN) {
        (
            StatusCode::FORBIDDEN,
            "Browser origins are not allowed on this endpoint",
        )
            .into_response()
    } else if !write && !read {
        ApiError(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "A valid Bearer token is required".into(),
        )
        .into_response()
    } else {
        request.extensions_mut().insert(CanWrite(write));
        next.run(request).await
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}

async fn no_stream() -> impl IntoResponse {
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")])
}

fn fault(id: Value, code: i32, message: &str) -> Response {
    let status = if id.is_null() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})),
    )
        .into_response()
}

async fn exchange(
    State(state): State<Arc<Service>>,
    Extension(CanWrite(write)): Extension<CanWrite>,
    headers: HeaderMap,
    body: std::result::Result<Json<Value>, JsonRejection>,
) -> Response {
    let mut versions = headers.get_all("mcp-protocol-version").iter();
    if let Some(version) = versions.next() {
        if versions.next().is_some()
            || !version
                .to_str()
                .is_ok_and(|version| SUPPORTED.contains(&version))
        {
            return fault(Value::Null, -32600, "Unsupported MCP protocol version");
        }
    }
    let Json(request) = match body {
        Ok(body) => body,
        Err(error) => return (error.status(), Json(json!({
            "jsonrpc":"2.0", "id":null, "error":{"code":-32700,"message":"Expected a JSON request within 4 MiB"}
        }))).into_response(),
    };
    let id = request.get("id").cloned();
    if request["jsonrpc"] != "2.0"
        || id
            .as_ref()
            .is_some_and(|id| !(id.is_string() || id.is_i64() || id.is_u64()))
    {
        return fault(Value::Null, -32600, "Invalid JSON-RPC request");
    }
    let Some(method) = request["method"].as_str() else {
        if id.is_some() && (request.get("result").is_some() ^ request.get("error").is_some()) {
            return StatusCode::ACCEPTED.into_response();
        }
        return fault(Value::Null, -32600, "Missing JSON-RPC method");
    };
    let Some(id) = id else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match method {
        "initialize" => {
            let Some(version) = request["params"]["protocolVersion"].as_str() else {
                return fault(id, -32602, "A protocol version is required");
            };
            let version = if SUPPORTED.contains(&version) {
                version
            } else {
                PROTOCOL
            };
            json!({"protocolVersion":version, "capabilities":{"tools":{}}, "serverInfo":{
                "name":"qingyu","title":"轻羽","version":env!("CARGO_PKG_VERSION")
            }})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools(write)}),
        "tools/call" => {
            let params = &request["params"];
            let Some(tool) = params["name"].as_str() else {
                return fault(id, -32602, "Missing tool name");
            };
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !args.is_object() {
                return fault(id, -32602, "Tool arguments must be an object");
            }
            match call(state, write, tool, args).await {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"structuredContent":value})
                }
                Err(ApiError(_, "unknown_tool", message)) => return fault(id, -32602, &message),
                Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, _, _)) => {
                    return fault(id, -32603, "The operation could not be completed");
                }
                Err(ApiError(status, code, message)) => json!({
                    "content":[{"type":"text","text":format!("{code} ({}): {message}", status.as_u16())}],
                    "isError":true
                }),
            }
        }
        _ => return fault(id, -32601, "Unknown JSON-RPC method"),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

fn tools(write: bool) -> Vec<Value> {
    let tool = |name, description, properties: Value, required: &[&str], read_only| {
        json!({
            "name":name, "description":description,
            "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},
            "annotations":{"readOnlyHint":read_only, "destructiveHint":name == "update_note"}
        })
    };
    let mut tools = vec![
        tool(
            "list_folders",
            "List QingYu note folders.",
            json!({}),
            &[],
            true,
        ),
        tool(
            "search_notes",
            "Find notes by title or Markdown text. Pass nextCursor as cursor for the next page.",
            json!({"q":{"type":"string"}, "folderId":{"type":"string"}, "cursor":{"type":"string"},
                   "limit":{"type":"integer","minimum":1,"maximum":200}}),
            &[],
            true,
        ),
        tool(
            "read_note",
            "Read one note and its version.",
            json!({"id":{"type":"string"}}),
            &["id"],
            true,
        ),
    ];
    if write {
        tools.push(tool("create_note", "Create a Markdown note in a folder returned by list_folders.",
            json!({"folderId":{"type":"string"},"title":{"type":"string"},"source":{"type":"string"}}),
            &["folderId","title","source"], false));
        tools.push(tool("update_note", "Replace a note's Markdown using its current version from read_note. Re-read and reconcile if the version changed.",
            json!({"id":{"type":"string"},"version":{"type":"string"},"title":{"type":"string"},"source":{"type":"string"}}),
            &["id","version","source"], false));
    }
    tools
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteId {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateArgs {
    id: String,
    version: String,
    title: Option<String>,
    source: String,
}

fn arguments<T: serde::de::DeserializeOwned>(value: Value) -> ApiResult<T> {
    serde_json::from_value(value).map_err(|_| ApiError::bad_request("Invalid tool arguments"))
}

async fn call(state: Arc<Service>, write: bool, tool: &str, args: Value) -> ApiResult<Value> {
    let state = State(state);
    let response = match tool {
        "list_folders" if args.as_object().is_some_and(|args| args.is_empty()) => {
            return Ok(folders(state).await?.0)
        }
        "list_folders" => return Err(ApiError::bad_request("This tool takes no arguments")),
        "search_notes" => return Ok(notes(state, Ok(Query(arguments(args)?))).await?.0),
        "read_note" => read(state, RoutePath(arguments::<NoteId>(args)?.id)).await?,
        "create_note" if write => create(state, Ok(Json(arguments(args)?))).await?,
        "update_note" if write => {
            let UpdateArgs {
                id,
                version,
                title,
                source,
            } = arguments(args)?;
            let mut headers = HeaderMap::new();
            headers.insert(
                header::IF_MATCH,
                format!("\"{version}\"")
                    .parse()
                    .map_err(|_| ApiError::bad_request("Invalid document version"))?,
            );
            update(
                state,
                RoutePath(id),
                headers,
                Ok(Json(UpdateNote { source, title })),
            )
            .await?
        }
        "create_note" | "update_note" => {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "read_only_token",
                "This token only permits reading".into(),
            ))
        }
        _ => {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "unknown_tool",
                "Unknown tool".into(),
            ))
        }
    };
    // The existing note endpoints already produce a single, in-memory JSON document.
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| ApiError::internal())?;
    serde_json::from_slice(&bytes).map_err(|_| ApiError::internal())
}
