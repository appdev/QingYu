use qingyu_kernel::{
    api::ApiDoc,
    contract::{
        ChangeServerOwnerPasswordRequest, CreateServerSessionRequest, ErrorCode, ErrorDetails,
        InitializeServerOwnerRequest, PositiveSafeInteger, ServerAuthenticationSecret,
        ServerAuthenticationStatusDto, ServerInitializationState, ServerSessionDto,
        ServerSessionState, MAX_SAFE_INTEGER,
    },
    error::{http_status_for_error_code, safe_error_envelope},
};
use serde_json::{json, Value};
use static_assertions::{assert_impl_all, assert_not_impl_any};
use uuid::Uuid;

assert_not_impl_any!(InitializeServerOwnerRequest: serde::Serialize, Clone);
assert_not_impl_any!(CreateServerSessionRequest: serde::Serialize, Clone);
assert_not_impl_any!(ChangeServerOwnerPasswordRequest: serde::Serialize, Clone);
assert_not_impl_any!(ServerAuthenticationSecret: serde::Serialize, Clone);
assert_impl_all!(ServerAuthenticationSecret: zeroize::ZeroizeOnDrop);

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
    let (initialization_token, owner_password): (
        ServerAuthenticationSecret,
        ServerAuthenticationSecret,
    ) = initialize.into_parts();
    assert_eq!(initialization_token.expose_secret(), "one-time-token");
    assert_eq!(owner_password.expose_secret(), "owner-password");
    assert_eq!(
        format!("{initialization_token:?} {owner_password:?}"),
        "ServerAuthenticationSecret([REDACTED]) ServerAuthenticationSecret([REDACTED])"
    );

    let session: CreateServerSessionRequest =
        serde_json::from_value(json!({ "password": "owner-password" })).unwrap();
    let owner_password: ServerAuthenticationSecret = session.into_password();
    assert_eq!(owner_password.expose_secret(), "owner-password");
    assert_eq!(
        format!("{owner_password:?}"),
        "ServerAuthenticationSecret([REDACTED])"
    );

    let password: ChangeServerOwnerPasswordRequest = serde_json::from_value(json!({
        "currentPassword": "current-owner-password",
        "newPassword": "new-owner-password",
    }))
    .unwrap();
    let (current_password, new_password): (ServerAuthenticationSecret, ServerAuthenticationSecret) =
        password.into_parts();
    assert_eq!(current_password.expose_secret(), "current-owner-password");
    assert_eq!(new_password.expose_secret(), "new-owner-password");
    assert_eq!(
        format!("{current_password:?} {new_password:?}"),
        "ServerAuthenticationSecret([REDACTED]) ServerAuthenticationSecret([REDACTED])"
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

    let request_id = qingyu_kernel::contract::RequestId::new(Uuid::nil());
    assert!(safe_error_envelope(ErrorCode::AuthenticationRateLimited, request_id, None).is_err());
    assert!(safe_error_envelope(ErrorCode::Unauthorized, request_id, None).is_ok());

    let details = ErrorDetails::RateLimit {
        retry_after_seconds: PositiveSafeInteger::new(31).unwrap(),
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
        json!([
            { "browserSessionHttps": [] },
            { "browserSessionHttp": [] }
        ])
    );
    for (method, path) in [
        ("post", "/api/v1/auth/logout"),
        ("patch", "/api/v1/auth/password"),
    ] {
        assert_eq!(
            document["paths"][path][method]["security"],
            json!([
                { "browserSessionHttps": [], "csrfTokenHttps": [] },
                { "browserSessionHttp": [], "csrfTokenHttp": [] }
            ])
        );
    }

    assert_eq!(
        document["components"]["securitySchemes"]["browserSessionHttps"],
        json!({ "type": "apiKey", "in": "cookie", "name": "__Host-qingyu_session" })
    );
    assert_eq!(
        document["components"]["securitySchemes"]["browserSessionHttp"],
        json!({ "type": "apiKey", "in": "cookie", "name": "qingyu_session" })
    );
    assert_eq!(
        document["components"]["securitySchemes"]["csrfTokenHttps"],
        json!({
            "type": "apiKey",
            "in": "header",
            "name": "X-CSRF-Token",
            "x-csrf-cookie-name": "__Host-qingyu_csrf",
        })
    );
    assert_eq!(
        document["components"]["securitySchemes"]["csrfTokenHttp"],
        json!({
            "type": "apiKey",
            "in": "header",
            "name": "X-CSRF-Token",
            "x-csrf-cookie-name": "qingyu_csrf",
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

    for (method, path) in [
        ("post", "/api/v1/auth/initialize"),
        ("post", "/api/v1/auth/session"),
        ("patch", "/api/v1/auth/password"),
    ] {
        let retry_after =
            &document["paths"][path][method]["responses"]["429"]["headers"]["Retry-After"];
        assert_eq!(retry_after["required"], true);
        assert_eq!(
            retry_after["schema"]["$ref"],
            "#/components/schemas/PositiveSafeInteger"
        );

        let rate_limit_response = &document["paths"][path][method]["responses"]["429"]["content"]
            ["application/json"]["schema"]["allOf"][1];
        assert_eq!(rate_limit_response["required"], json!(["code", "details"]));
        assert_eq!(
            rate_limit_response["properties"]["details"]["required"],
            json!(["type", "retryAfterSeconds"])
        );
        assert_eq!(
            rate_limit_response["properties"]["details"]["properties"]["retryAfterSeconds"]["$ref"],
            "#/components/schemas/PositiveSafeInteger"
        );
    }

    assert_eq!(
        document["components"]["schemas"]["PositiveSafeInteger"]["minimum"],
        1
    );
    assert_eq!(
        document["components"]["schemas"]["PositiveSafeInteger"]["maximum"],
        MAX_SAFE_INTEGER
    );
    assert_eq!(
        document["x-cors-exposed-response-headers"],
        json!(["Retry-After", "X-Request-Id"])
    );
}

fn assert_request_schema(document: &Value, method: &str, path: &str, schema: &str) {
    assert_eq!(
        document["paths"][path][method]["requestBody"]["content"]["application/json"]["schema"]
            ["$ref"],
        format!("#/components/schemas/{schema}")
    );
}
