//! 网页客户端的轻羽账号。逻辑和 App 共用 crate::client_account，这里只把 HTTP 请求转给它，
//! 会话保存在部署时指定的配置目录里。路由挂在 browser_library 下，需要先用访问密码解锁。
use super::*;
use crate::client_account::{portal_url, AccountError, ClientAccount};
use axum::response::Redirect;
use serde::Deserialize;

pub(super) fn routes() -> Router<Arc<Service>> {
    Router::new()
        .route("/web/account", get(status))
        .route("/web/account/login", axum::routing::post(login))
        .route("/web/account/logout", axum::routing::post(logout))
        .route("/web/account/redeem", axum::routing::post(redeem))
        .route("/web/account/redemptions", get(redemptions))
        .route("/web/account/portal", get(portal))
}

/// 账号错误原样交给前端，和 App 里 invoke 失败时拿到的值一致。
struct Failure(StatusCode, serde_json::Value);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<AccountError> for Failure {
    fn from(error: AccountError) -> Self {
        Self(
            StatusCode::BAD_REQUEST,
            serde_json::to_value(error).unwrap_or(json!("unknown")),
        )
    }
}

type AccountResult<T> = std::result::Result<Json<T>, Failure>;

fn session(state: &Service) -> std::result::Result<PathBuf, Failure> {
    state
        .config
        .as_deref()
        .map(ClientAccount::session_path)
        .ok_or_else(|| Failure(StatusCode::SERVICE_UNAVAILABLE, json!("config_required")))
}

fn body<T>(value: std::result::Result<Json<T>, JsonRejection>) -> std::result::Result<T, Failure> {
    value
        .map(|Json(value)| value)
        .map_err(|_| Failure(StatusCode::BAD_REQUEST, json!("invalid_input")))
}

#[derive(Deserialize)]
struct Force {
    #[serde(default)]
    force: bool,
}

async fn status(
    State(state): State<Arc<Service>>,
    Query(query): Query<Force>,
) -> AccountResult<Option<crate::client_account::AccountInfo>> {
    Ok(Json(state.account.status(session(&state)?, query.force).await?))
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<Service>>,
    request: std::result::Result<Json<Login>, JsonRejection>,
) -> AccountResult<crate::client_account::AccountInfo> {
    let Login { username, password } = body(request)?;
    Ok(Json(state.account.login(session(&state)?, &username, &password).await?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logout {
    account_id: String,
    everywhere: bool,
}

async fn logout(
    State(state): State<Arc<Service>>,
    request: std::result::Result<Json<Logout>, JsonRejection>,
) -> AccountResult<bool> {
    let Logout {
        account_id,
        everywhere,
    } = body(request)?;
    Ok(Json(
        state
            .account
            .logout(session(&state)?, &account_id, everywhere, || Ok(()))
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Redeem {
    account_id: String,
    activation_code: String,
}

async fn redeem(
    State(state): State<Arc<Service>>,
    request: std::result::Result<Json<Redeem>, JsonRejection>,
) -> AccountResult<u64> {
    let Redeem {
        account_id,
        activation_code,
    } = body(request)?;
    Ok(Json(
        state
            .account
            .redeem(session(&state)?, &account_id, &activation_code)
            .await?,
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Redemptions {
    account_id: String,
    #[serde(default)]
    force: bool,
}

async fn redemptions(
    State(state): State<Arc<Service>>,
    Query(query): Query<Redemptions>,
) -> AccountResult<Vec<crate::client_account::Redemption>> {
    Ok(Json(
        state
            .account
            .redemptions(session(&state)?, &query.account_id, query.force)
            .await?,
    ))
}

#[derive(Deserialize)]
struct Portal {
    page: String,
}

/// 门户在新标签页打开：前端直接打开这个地址，由这里跳转到轻羽门户，地址表和 App 共用。
async fn portal(Query(query): Query<Portal>) -> std::result::Result<Redirect, Failure> {
    Ok(Redirect::to(&portal_url(&query.page)?))
}
