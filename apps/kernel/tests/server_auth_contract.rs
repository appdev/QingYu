use qingyu_kernel::{
    api::ApiDoc,
    contract::{
        ChangeServerOwnerPasswordRequest, CreateServerSessionRequest, ErrorCode, ErrorDetails,
        InitializeServerOwnerRequest, SafeUnsignedInteger, ServerAuthenticationStatusDto,
        ServerInitializationState, ServerSessionDto, ServerSessionState,
    },
    error::{http_status_for_error_code, safe_error_envelope},
};
use serde_json::{json, Value};
use static_assertions::assert_not_impl_any;
use uuid::Uuid;

assert_not_impl_any!(InitializeServerOwnerRequest: serde::Serialize, Clone);
assert_not_impl_any!(CreateServerSessionRequest: serde::Serialize, Clone);
assert_not_impl_any!(ChangeServerOwnerPasswordRequest: serde::Serialize, Clone);

#[test]
fn server_authentication_dtos_freeze_exact_camel_case_wire_shapes() {
    assert_eq!(
        serde_json::to_value(ServerAuthenticationStatusDto {
            initialization: ServerInitializationState::Required,
        })
        .unwrap(),
        json!({ "initialization": "required" })
    );
    assert_eq!(
        serde_json::to_value(ServerSessionDto {
            state: ServerSessionState::Authenticated,
        })
        .unwrap(),
        json!({ "state": "authenticated" })
    );

    let initialize: InitializeServerOwnerRequest = serde_json::from_value(json!({
        "initializationToken": "one-time-token",
        "password": "owner-password",
    }))
    .unwrap();
    assert_eq!(
        initialize.into_parts(),
        ("one-time-token".to_owned(), "owner-password".to_owned())
    );

    let session: CreateServerSessionRequest =
        serde_json::from_value(json!({ "password": "owner-password" })).unwrap();
    assert_eq!(session.into_password(), "owner-password");

    let password: ChangeServerOwnerPasswordRequest = serde_json::from_value(json!({
        "currentPassword": "current-owner-password",
        "newPassword": "new-owner-password",
    }))
    .unwrap();
    assert_eq!(
        password.into_parts(),
        (
            "current-owner-password".to_owned(),
            "new-owner-password".to_owned()
        )
    );
}

#[test]
fn server_authentication_secret_requests_reject_unknown_fields_and_redact_debug() {
    for invalid in [
        json!({
            "initializationToken": "one-time-token",
            "password": "owner-password",
            "extra": true,
        }),
        json!({ "password": "owner-password", "extra": true }),
        json!({
            "currentPassword": "current-owner-password",
            "newPassword": "new-owner-password",
            "extra": true,
        }),
    ] {
        let rendered = invalid.to_string();
        let rejected = serde_json::from_str::<InitializeServerOwnerRequest>(&rendered).is_err()
            && serde_json::from_str::<CreateServerSessionRequest>(&rendered).is_err()
            && serde_json::from_str::<ChangeServerOwnerPasswordRequest>(&rendered).is_err();
        assert!(rejected);
    }

    let initialize: InitializeServerOwnerRequest = serde_json::from_value(json!({
        "initializationToken": "one-time-token",
        "password": "owner-password",
    }))
    .unwrap();
    let session: CreateServerSessionRequest =
        serde_json::from_value(json!({ "password": "owner-password" })).unwrap();
    let password: ChangeServerOwnerPasswordRequest = serde_json::from_value(json!({
        "currentPassword": "current-owner-password",
        "newPassword": "new-owner-password",
    }))
    .unwrap();
    let rendered = format!("{initialize:?} {session:?} {password:?}");
    for secret in [
        "one-time-token",
        "owner-password",
        "current-owner-password",
        "new-owner-password",
    ] {
        assert!(!rendered.contains(secret));
    }
}

#[test]
fn server_authentication_errors_have_stable_statuses_and_safe_rate_limit_details() {
    let expected = [
        (ErrorCode::InitializationRequired, 409),
        (ErrorCode::AlreadyInitialized, 409),
        (ErrorCode::InvalidCredentials, 401),
        (ErrorCode::CsrfRejected, 403),
        (ErrorCode::AuthenticationRateLimited, 429),
        (ErrorCode::AuthenticationUnavailable, 503),
    ];
    for (code, status) in expected {
        assert_eq!(http_status_for_error_code(code), status);
    }

    let details = ErrorDetails::RateLimit {
        retry_after_seconds: SafeUnsignedInteger::new(31).unwrap(),
    };
    let envelope = safe_error_envelope(
        ErrorCode::AuthenticationRateLimited,
        qingyu_kernel::contract::RequestId::new(Uuid::nil()),
        Some(details),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(envelope).unwrap(),
        json!({
            "code": "authentication_rate_limited",
            "message": "Authentication is temporarily limited.",
            "requestId": Uuid::nil(),
            "details": {
                "type": "rate-limit",
                "retryAfterSeconds": 31,
            },
        })
    );
}

#[test]
fn openapi_freezes_server_auth_routes_and_browser_security_composition() {
    let document = serde_json::to_value(ApiDoc::openapi()).unwrap();
    let expected = [
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
    ];
    for (method, path, operation_id) in expected {
        assert_eq!(document["paths"][path][method]["operationId"], operation_id);
    }

    for (method, path) in [
        ("get", "/api/v1/auth/status"),
        ("post", "/api/v1/auth/initialize"),
        ("post", "/api/v1/auth/session"),
    ] {
        assert!(document["paths"][path][method].get("security").is_none());
    }
    assert_eq!(
        document["paths"]["/api/v1/auth/session"]["get"]["security"],
        json!([{ "browserSession": [] }])
    );
    for (method, path) in [
        ("post", "/api/v1/auth/logout"),
        ("patch", "/api/v1/auth/password"),
    ] {
        assert_eq!(
            document["paths"][path][method]["security"],
            json!([{ "browserSession": [], "csrfToken": [] }])
        );
    }

    assert_eq!(
        document["components"]["securitySchemes"]["browserSession"],
        json!({ "type": "apiKey", "in": "cookie", "name": "__Host-qingyu_session" })
    );
    assert_eq!(
        document["components"]["securitySchemes"]["csrfToken"],
        json!({
            "type": "apiKey",
            "in": "header",
            "name": "X-CSRF-Token",
            "x-csrf-cookie-name": "__Host-qingyu_csrf",
        })
    );

    assert_request_schema(
        &document,
        "post",
        "/api/v1/auth/initialize",
        "InitializeServerOwnerRequest",
    );
    assert_request_schema(
        &document,
        "post",
        "/api/v1/auth/session",
        "CreateServerSessionRequest",
    );
    assert_request_schema(
        &document,
        "patch",
        "/api/v1/auth/password",
        "ChangeServerOwnerPasswordRequest",
    );

    let initialize = &document["components"]["schemas"]["InitializeServerOwnerRequest"];
    assert_eq!(
        initialize["properties"]["initializationToken"]["writeOnly"],
        true
    );
    assert_eq!(initialize["properties"]["password"]["writeOnly"], true);
    let change = &document["components"]["schemas"]["ChangeServerOwnerPasswordRequest"];
    assert_eq!(change["properties"]["currentPassword"]["writeOnly"], true);
    assert_eq!(change["properties"]["newPassword"]["writeOnly"], true);
}

fn assert_request_schema(document: &Value, method: &str, path: &str, schema: &str) {
    assert_eq!(
        document["paths"][path][method]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        format!("#/components/schemas/{schema}")
    );
}
