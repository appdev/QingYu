use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use qingyu_kernel::api::{check_openapi_artifact, export_openapi_to_string, ApiDoc};
use qingyu_kernel::contract::ErrorCode;
use qingyu_kernel::error::http_status_for_error_code;
use serde_json::Value;
use tempfile::tempdir;

const HTTP_OPERATIONS: &[(&str, &str, &str)] = &[
    ("get", "/api/v1/auth/status", "getAuthenticationStatus"),
    ("post", "/api/v1/auth/initialize", "initializeServerOwner"),
    ("post", "/api/v1/auth/session", "createServerSession"),
    ("get", "/api/v1/auth/session", "getServerSession"),
    ("post", "/api/v1/auth/logout", "logoutServerSession"),
    (
        "patch",
        "/api/v1/auth/password",
        "changeServerOwnerPassword",
    ),
    ("get", "/api/v1/health/live", "healthLive"),
    ("get", "/api/v1/health/ready", "healthReady"),
    ("get", "/api/v1/system/version", "getSystemVersion"),
    ("get", "/api/v1/runtime", "getRuntimeState"),
    ("get", "/api/v1/workspace", "getWorkspace"),
    ("get", "/api/v1/inventory", "listWorkspaceInventory"),
    (
        "get",
        "/api/v1/resources/{resourceId}",
        "openWorkspaceResource",
    ),
    ("get", "/api/v1/documents", "listDocuments"),
    ("post", "/api/v1/documents", "createDocument"),
    ("get", "/api/v1/documents/{documentId}", "getDocument"),
    ("put", "/api/v1/documents/{documentId}", "updateDocument"),
    (
        "post",
        "/api/v1/documents/{documentId}/move",
        "moveDocument",
    ),
    (
        "post",
        "/api/v1/documents/{documentId}/delete",
        "deleteDocument",
    ),
    (
        "get",
        "/api/v1/documents/{documentId}/history",
        "listDocumentHistory",
    ),
    (
        "get",
        "/api/v1/documents/{documentId}/history/{snapshotId}",
        "getDocumentHistory",
    ),
    (
        "post",
        "/api/v1/documents/{documentId}/history/{snapshotId}/restore",
        "restoreDocumentHistory",
    ),
    ("get", "/api/v1/search", "searchWorkspace"),
    ("get", "/api/v1/settings", "getSettings"),
    ("patch", "/api/v1/settings", "patchSettings"),
    ("get", "/api/v1/sync/config", "getSyncConfig"),
    ("patch", "/api/v1/sync/config", "patchSyncConfig"),
    ("post", "/api/v1/sync/connection-test", "testSyncConnection"),
    ("get", "/api/v1/sync/status", "getSyncStatus"),
    ("post", "/api/v1/sync/runs", "triggerSyncRun"),
];

const ERROR_CODES: &[&str] = &[
    "invalid_request",
    "invalid_workspace_path",
    "invalid_document_name",
    "unauthorized",
    "initialization_required",
    "already_initialized",
    "invalid_credentials",
    "csrf_rejected",
    "authentication_rate_limited",
    "authentication_unavailable",
    "host_not_allowed",
    "origin_not_allowed",
    "kernel_not_ready",
    "workspace_unavailable",
    "workspace_locked",
    "document_not_found",
    "resource_not_found",
    "document_already_exists",
    "document_too_large",
    "document_invalid_encoding",
    "revision_conflict",
    "settings_revision_conflict",
    "sync_config_revision_conflict",
    "invalid_settings_field",
    "settings_unavailable",
    "sync_config_absent",
    "sync_config_invalid",
    "sync_not_ready",
    "sync_run_unavailable",
    "internal_error",
];

const WS_COMPONENTS: &[&str] = &[
    "AuthenticateFrame",
    "ReadyFrame",
    "EventFrame",
    "GapFrame",
    "ErrorFrame",
    "ServerFrame",
    "ResourceRefDto",
    "WorkspaceChangedEvent",
    "DocumentCreatedEvent",
    "DocumentChangedEvent",
    "DocumentMovedEvent",
    "DocumentDeletedEvent",
    "SettingsChangedEvent",
    "SyncConfigChangedEvent",
    "SyncStatusChangedEvent",
    "DomainEvent",
];

fn api_document() -> Value {
    serde_json::to_value(ApiDoc::openapi()).expect("OpenAPI must serialize")
}

fn components(document: &Value) -> &serde_json::Map<String, Value> {
    document
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .expect("OpenAPI components.schemas")
}

fn component<'a>(document: &'a Value, name: &str) -> &'a Value {
    components(document)
        .get(name)
        .unwrap_or_else(|| panic!("missing OpenAPI component {name}"))
}

fn resolve_schema<'a>(document: &'a Value, schema: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let name = reference
        .strip_prefix("#/components/schemas/")
        .expect("schema references must stay inside components");
    component(document, name)
}

fn property<'a>(document: &'a Value, schema_name: &str, field: &str) -> &'a Value {
    let schema = component(document, schema_name);
    schema_property(document, schema, field)
        .unwrap_or_else(|| panic!("{schema_name}.{field} must be present"))
}

fn schema_property<'a>(document: &'a Value, schema: &'a Value, field: &str) -> Option<&'a Value> {
    let schema = resolve_schema(document, schema);
    if let Some(property) = schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(field))
    {
        return Some(property);
    }
    schema
        .get("allOf")
        .and_then(Value::as_array)
        .and_then(|parts| {
            parts
                .iter()
                .find_map(|part| schema_property(document, part, field))
        })
}

fn is_required(document: &Value, schema_name: &str, field: &str) -> bool {
    schema_requires(document, component(document, schema_name), field)
}

fn schema_requires(document: &Value, schema: &Value, field: &str) -> bool {
    let schema = resolve_schema(document, schema);
    schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| required.iter().any(|name| name.as_str() == Some(field)))
        || schema
            .get("allOf")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| schema_requires(document, part, field))
            })
}

fn allows_null(document: &Value, schema: &Value) -> bool {
    let schema = resolve_schema(document, schema);
    if schema.get("nullable") == Some(&Value::Bool(true))
        || schema.get("type") == Some(&Value::String("null".to_owned()))
        || schema.get("const").is_some_and(Value::is_null)
    {
        return true;
    }
    if schema
        .get("type")
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|value| value.as_str() == Some("null")))
        || schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(Value::is_null))
    {
        return true;
    }
    ["oneOf", "anyOf"]
        .iter()
        .filter_map(|key| schema.get(*key).and_then(Value::as_array))
        .flatten()
        .any(|part| allows_null(document, part))
}

fn assert_required_nullable(document: &Value, schema: &str, field: &str) {
    assert!(
        is_required(document, schema, field),
        "{schema}.{field} must be required"
    );
    assert!(
        allows_null(document, property(document, schema, field)),
        "{schema}.{field} must explicitly allow null"
    );
}

fn assert_optional_non_null(document: &Value, schema: &str, field: &str) {
    assert!(
        !is_required(document, schema, field),
        "{schema}.{field} must be optional"
    );
    assert!(
        !allows_null(document, property(document, schema, field)),
        "{schema}.{field} must be omitted, not null"
    );
}

#[test]
fn openapi_has_exactly_the_frozen_thirty_http_operations() {
    let document = api_document();
    let paths = document["paths"].as_object().expect("OpenAPI paths");
    assert!(
        !paths.contains_key("/api/v1/events"),
        "the WebSocket upgrade is deliberately not an OpenAPI path"
    );

    let expected: BTreeMap<(&str, &str), &str> = HTTP_OPERATIONS
        .iter()
        .map(|(method, path, operation)| ((*method, *path), *operation))
        .collect();
    assert_eq!(expected.len(), 30);

    let mut actual = BTreeMap::new();
    for (path, path_item) in paths {
        let path_item = path_item.as_object().expect("path item must be an object");
        for method in ["get", "post", "put", "patch", "delete"] {
            if let Some(operation) = path_item.get(method) {
                let operation_id = operation
                    .get("operationId")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{method} {path} is missing operationId"));
                actual.insert((method, path.as_str()), operation_id);
            }
        }
    }
    assert_eq!(actual, expected);
}

#[test]
fn public_auth_bootstrap_and_dual_host_security_are_explicit_per_operation() {
    let document = api_document();
    assert!(
        document.get("security").is_none(),
        "authentication must be explicit per operation"
    );

    for (method, path, operation_id) in HTTP_OPERATIONS {
        let operation = &document["paths"][*path][*method];
        match *operation_id {
            "healthLive"
            | "getAuthenticationStatus"
            | "initializeServerOwner"
            | "createServerSession" => assert!(
                operation.get("security").is_none(),
                "{operation_id} must be public"
            ),
            "getServerSession" => assert_eq!(
                operation.get("security"),
                Some(&serde_json::json!([
                    { "browserSessionHttps": [] },
                    { "browserSessionHttp": [] }
                ])),
                "getServerSession must require the browser session"
            ),
            "logoutServerSession" | "changeServerOwnerPassword" => assert_eq!(
                operation.get("security"),
                Some(&serde_json::json!([
                    { "browserSessionHttps": [], "csrfTokenHttps": [] },
                    { "browserSessionHttp": [], "csrfTokenHttp": [] }
                ])),
                "{operation_id} must require browser session and CSRF"
            ),
            _ if *method == "get" => assert_eq!(
                operation.get("security"),
                Some(&serde_json::json!([
                    { "nativeBearer": [] },
                    { "browserSessionHttps": [] },
                    { "browserSessionHttp": [] }
                ])),
                "{operation_id} must accept native bearer or browser session"
            ),
            _ => assert_eq!(
                operation.get("security"),
                Some(&serde_json::json!([
                    { "nativeBearer": [] },
                    { "browserSessionHttps": [], "csrfTokenHttps": [] },
                    { "browserSessionHttp": [], "csrfTokenHttp": [] }
                ])),
                "{operation_id} browser mutations must require CSRF"
            ),
        }
    }

    let native_bearer = &document["components"]["securitySchemes"]["nativeBearer"];
    assert_eq!(native_bearer["type"], "http");
    assert_eq!(native_bearer["scheme"], "bearer");
    for (scheme, cookie_name) in [
        ("browserSessionHttps", "__Host-qingyu_session"),
        ("browserSessionHttp", "qingyu_session"),
    ] {
        let browser_session = &document["components"]["securitySchemes"][scheme];
        assert_eq!(browser_session["type"], "apiKey");
        assert_eq!(browser_session["in"], "cookie");
        assert_eq!(browser_session["name"], cookie_name);
    }
    for (scheme, cookie_name) in [
        ("csrfTokenHttps", "__Host-qingyu_csrf"),
        ("csrfTokenHttp", "qingyu_csrf"),
    ] {
        let csrf = &document["components"]["securitySchemes"][scheme];
        assert_eq!(csrf["type"], "apiKey");
        assert_eq!(csrf["in"], "header");
        assert_eq!(csrf["name"], "X-CSRF-Token");
        assert_eq!(csrf["x-csrf-cookie-name"], cookie_name);
    }
}

#[test]
fn every_http_response_requires_the_request_id_header() {
    let document = api_document();

    for (method, path, operation_id) in HTTP_OPERATIONS {
        let responses = document["paths"][*path][*method]["responses"]
            .as_object()
            .unwrap_or_else(|| panic!("{operation_id} responses must be an object"));
        for (status, response) in responses {
            let header = &response["headers"]["X-Request-Id"];
            assert_eq!(
                header["required"], true,
                "{operation_id} {status} must require X-Request-Id"
            );
            assert_eq!(header["schema"]["type"], "string");
            assert_eq!(header["schema"]["format"], "uuid");
        }
    }
}

#[test]
fn every_operation_error_status_matches_the_runtime_error_mapping() {
    let document = api_document();

    for (method, path, operation_id) in HTTP_OPERATIONS {
        let responses = document["paths"][*path][*method]["responses"]
            .as_object()
            .unwrap_or_else(|| panic!("{operation_id} responses must be an object"));
        for (status, response) in responses {
            let Some(codes) = response
                .pointer("/content/application~1json/schema/allOf/1/properties/code/enum")
                .and_then(Value::as_array)
            else {
                continue;
            };
            for code in codes {
                let code: ErrorCode =
                    serde_json::from_value(code.clone()).expect("error code must deserialize");
                assert_eq!(
                    status
                        .parse::<u16>()
                        .expect("response status must be numeric"),
                    http_status_for_error_code(code),
                    "{operation_id} must publish the runtime status for {code:?}"
                );
            }
        }
    }
}

#[test]
fn websocket_frames_are_components_but_the_upgrade_is_not_a_path() {
    let document = api_document();
    assert!(document["paths"].get("/api/v1/events").is_none());
    for schema in WS_COMPONENTS {
        assert!(
            components(&document).contains_key(*schema),
            "missing WebSocket component {schema}"
        );
    }
}

#[test]
fn enum_variant_fields_match_the_runtime_camel_case_wire_format() {
    let document = api_document();
    let rendered = serde_json::to_string(components(&document)).unwrap();
    for forbidden in [
        "workspace_generation",
        "modified_at",
        "size_bytes",
        "previous_path",
        "document_id",
        "current_revision",
        "protocol_version",
        "connection_id",
        "instance_id",
        "snapshot_required",
        "reload_scopes",
    ] {
        assert!(
            !rendered.contains(&format!("\"{forbidden}\":")),
            "schema property must not use Rust field name {forbidden}"
        );
    }
    for required in [
        "workspaceGeneration",
        "modifiedAt",
        "sizeBytes",
        "previousPath",
        "documentId",
        "currentRevision",
        "protocolVersion",
        "connectionId",
        "instanceId",
        "snapshotRequired",
        "reloadScopes",
    ] {
        assert!(
            rendered.contains(&format!("\"{required}\":")),
            "schema property must expose runtime field name {required}"
        );
    }
}

#[test]
fn document_contents_schema_freezes_the_decoded_size_limit() {
    let document = api_document();
    assert_eq!(
        component(&document, "DocumentContents")["x-max-utf8-bytes"],
        16 * 1024 * 1024
    );
}

#[test]
fn private_capability_roots_and_absolute_host_paths_never_enter_the_schema() {
    let document = api_document();
    let rendered = serde_json::to_string(&document).expect("OpenAPI JSON");
    for private_type in ["WorkspaceRoot", "InstanceDataRoot", "CacheRoot", "PathBuf"] {
        assert!(
            !rendered.contains(private_type),
            "private host type leaked into OpenAPI: {private_type}"
        );
    }

    let forbidden_fields: BTreeSet<&str> = [
        "absolutePath",
        "absoluteRoot",
        "hostPath",
        "hostRoot",
        "workspaceRoot",
        "instanceDataRoot",
        "cacheRoot",
        "pathBuf",
    ]
    .into_iter()
    .collect();
    for (schema_name, schema) in components(&document) {
        assert_no_forbidden_properties(schema_name, schema, &forbidden_fields);
    }
}

fn assert_no_forbidden_properties(
    schema_name: &str,
    value: &Value,
    forbidden_fields: &BTreeSet<&str>,
) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for field in properties.keys() {
                    assert!(
                        !forbidden_fields.contains(field.as_str()),
                        "{schema_name}.{field} exposes a private or absolute host path"
                    );
                }
            }
            for child in object.values() {
                assert_no_forbidden_properties(schema_name, child, forbidden_fields);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_no_forbidden_properties(schema_name, child, forbidden_fields);
            }
        }
        _ => {}
    }
}

#[test]
fn nullable_fields_are_required_while_optional_fields_are_omitted_not_null() {
    let document = api_document();

    for page in [
        "DocumentPageDto",
        "DocumentHistoryPageDto",
        "SearchPageDto",
        "WorkspaceInventoryPageDto",
    ] {
        assert_required_nullable(&document, page, "nextCursor");
    }
    assert_required_nullable(&document, "SafeEndpointViewDto", "value");
    for field in [
        "configRevision",
        "activeRunId",
        "lastAttemptAt",
        "lastSuccessfulSyncAt",
        "lastTrigger",
        "summary",
        "error",
    ] {
        assert_required_nullable(&document, "SyncStatusDto", field);
    }

    assert_optional_non_null(&document, "PageQuery", "cursor");
    assert_optional_non_null(&document, "PageQuery", "limit");
    assert_optional_non_null(&document, "ListDocumentsQuery", "parent");
    assert_optional_non_null(&document, "ListWorkspaceInventoryQuery", "cursor");
    assert_optional_non_null(&document, "ListWorkspaceInventoryQuery", "limit");
    assert_optional_non_null(&document, "ListWorkspaceInventoryQuery", "parent");
    assert_optional_non_null(&document, "ApiErrorEnvelope", "details");
    for field in ["category", "httpStatus", "requestId", "runId"] {
        assert_optional_non_null(&document, "SyncSafeErrorDto", field);
    }
}

#[test]
fn snapshot_required_is_the_literal_true_constant() {
    let document = api_document();
    let schema = resolve_schema(&document, component(&document, "SnapshotRequired"));
    assert_eq!(
        schema.get("type"),
        Some(&Value::String("boolean".to_owned()))
    );
    assert_eq!(schema.get("const"), Some(&Value::Bool(true)));

    assert!(is_required(&document, "ReadyFrame", "snapshotRequired"));
    let property = resolve_schema(
        &document,
        property(&document, "ReadyFrame", "snapshotRequired"),
    );
    assert_eq!(property.get("const"), Some(&Value::Bool(true)));
}

#[test]
fn stable_error_code_schema_contains_only_the_frozen_v1_codes() {
    let document = api_document();
    let actual: BTreeSet<&str> = component(&document, "ErrorCode")["enum"]
        .as_array()
        .expect("ErrorCode must be an enum")
        .iter()
        .map(|value| value.as_str().expect("error code must be a string"))
        .collect();
    let expected: BTreeSet<&str> = ERROR_CODES.iter().copied().collect();
    assert_eq!(actual, expected);
}

#[test]
fn operations_freeze_request_bodies_parameters_and_route_specific_errors() {
    let document = api_document();
    assert_eq!(
        document["paths"]["/api/v1/documents"]["post"]["requestBody"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/CreateDocumentRequest"
    );
    assert_eq!(
        document["paths"]["/api/v1/settings"]["patch"]["requestBody"]["content"]
            ["application/json"]["schema"]["$ref"],
        "#/components/schemas/PatchSettingsRequest"
    );
    let document_parameters = document["paths"]["/api/v1/documents/{documentId}"]["put"]
        ["parameters"]
        .as_array()
        .expect("document path parameters");
    assert!(document_parameters.iter().any(|parameter| {
        parameter["name"] == "documentId"
            && parameter["in"] == "path"
            && parameter["required"] == true
            && parameter["schema"]["$ref"] == "#/components/schemas/DocumentId"
    }));
    let search_parameters = document["paths"]["/api/v1/search"]["get"]["parameters"]
        .as_array()
        .expect("search query parameters");
    assert!(search_parameters.iter().any(|parameter| {
        parameter["name"] == "query"
            && parameter["in"] == "query"
            && parameter["required"] == true
            && parameter["schema"]["$ref"] == "#/components/schemas/SearchQuery"
    }));
    let resource_parameters = document["paths"]["/api/v1/resources/{resourceId}"]["get"]
        ["parameters"]
        .as_array()
        .expect("resource parameters");
    for (name, location, schema) in [
        ("resourceId", "path", "ResourceId"),
        ("kind", "query", "ResourceKind"),
    ] {
        assert!(resource_parameters.iter().any(|parameter| {
            parameter["name"] == name
                && parameter["in"] == location
                && parameter["required"] == true
                && parameter["schema"]["$ref"] == format!("#/components/schemas/{schema}")
        }));
    }
    let binary =
        &document["paths"]["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["content"];
    for media_type in [
        "application/octet-stream",
        "image/gif",
        "image/jpeg",
        "image/png",
        "image/webp",
    ] {
        assert_eq!(binary[media_type]["schema"]["type"], "string");
        assert_eq!(binary[media_type]["schema"]["format"], "binary");
    }
    let binary_headers =
        &document["paths"]["/api/v1/resources/{resourceId}"]["get"]["responses"]["200"]["headers"];
    assert_eq!(binary_headers["Content-Length"]["required"], true);
    assert_eq!(
        binary_headers["Content-Length"]["schema"]["type"],
        "integer"
    );
    assert_eq!(binary_headers["Content-Length"]["schema"]["minimum"], 0);
    assert_eq!(binary_headers["X-Content-Type-Options"]["required"], true);
    assert_eq!(
        binary_headers["X-Content-Type-Options"]["schema"]["const"],
        "nosniff"
    );

    let patch_settings_errors =
        operation_error_codes(&document["paths"]["/api/v1/settings"]["patch"]["responses"]);
    assert_eq!(
        patch_settings_errors,
        BTreeSet::from([
            "host_not_allowed",
            "authentication_unavailable",
            "csrf_rejected",
            "internal_error",
            "invalid_request",
            "invalid_settings_field",
            "origin_not_allowed",
            "settings_revision_conflict",
            "settings_unavailable",
            "unauthorized",
        ])
    );
}

fn operation_error_codes(responses: &Value) -> BTreeSet<&str> {
    responses
        .as_object()
        .expect("operation responses")
        .values()
        .filter_map(|response| {
            response
                .pointer("/content/application~1json/schema/allOf/1/properties/code/enum")
                .and_then(Value::as_array)
        })
        .flatten()
        .map(|code| code.as_str().expect("error code string"))
        .collect()
}

#[test]
fn export_is_deterministic_and_check_detects_one_byte_drift_without_repo_writes() {
    let first = export_openapi_to_string().expect("OpenAPI export");
    let second = export_openapi_to_string().expect("repeat OpenAPI export");
    assert_eq!(first, second, "OpenAPI export must be byte deterministic");
    assert!(
        first.ends_with('\n'),
        "checked-in JSON must end in one newline"
    );

    let repository_artifact = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("openapi")
        .join("kernel-v1.json");
    let artifact_before = fs::read(&repository_artifact).expect("checked-in OpenAPI artifact");
    assert_eq!(artifact_before, first.as_bytes());

    let directory = tempdir().expect("temporary artifact directory");
    let fixture = directory.path().join("kernel-v1.json");
    fs::write(&fixture, first.as_bytes()).expect("write matching fixture");
    check_openapi_artifact(&fixture).expect("matching fixture must pass --check");

    let mut drifted = first.into_bytes();
    let index = drifted
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .expect("export is not empty");
    drifted[index] ^= 1;
    fs::write(&fixture, drifted).expect("write one-byte drift fixture");
    assert!(
        check_openapi_artifact(&fixture).is_err(),
        "--check must reject a one-byte drift"
    );

    assert_eq!(
        fs::read(&repository_artifact).expect("re-read checked-in artifact"),
        artifact_before,
        "fixture drift checking must never rewrite the repository artifact"
    );
}
