//! Password-only browser access; API Bearer credentials remain independent.
use super::*;
use axum::{
    response::{Html, Redirect},
    routing::post,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use tower_http::services::ServeDir;

const SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: usize = 10;
const MAX_SESSIONS: usize = 256;

#[derive(Default)]
struct Sessions {
    active: HashMap<[u8; 32], Instant>,
    failures: VecDeque<Instant>,
}

pub(super) struct Web {
    directory: PathBuf,
    index: String,
    password: [u8; 32],
    secure: bool,
    sessions: Mutex<Sessions>,
}

impl Web {
    pub(super) fn open(directory: PathBuf, password: &str, secure: bool) -> Result<Self> {
        if password.chars().count() < 12 || password.len() > 1024 || password.trim().is_empty() {
            return Err("QINGYU_WEB_PASSWORD must contain at least 12 characters and at most 1024 UTF-8 bytes".into());
        }
        let directory = directory.canonicalize().map_err(error)?;
        let index = fs::read_to_string(directory.join("index.html")).map_err(error)?;
        if !index.contains("</head>") || !directory.join("login.html").is_file() {
            return Err("QINGYU_WEB_DIR must contain the built index.html and login.html".into());
        }
        Ok(Self {
            directory,
            index: index.replacen(
                "</head>",
                "<meta name=\"qingyu-web-auth\" content=\"enabled\"></head>",
                1,
            ),
            password: Sha256::digest(password.as_bytes()).into(),
            secure,
            sessions: Mutex::new(Sessions::default()),
        })
    }

    fn cookie_name(&self) -> &'static str {
        if self.secure {
            "__Host-qingyu_session"
        } else {
            "qingyu_session"
        }
    }

    fn cookie_hash(&self, headers: &HeaderMap) -> Option<[u8; 32]> {
        let mut cookies = headers
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .filter_map(|part| part.trim().split_once('='))
            .filter(|(name, _)| *name == self.cookie_name());
        let (_, value) = cookies.next()?;
        if cookies.next().is_some()
            || value.len() != 64
            || !value.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        Some(Sha256::digest(value.as_bytes()).into())
    }

    pub(super) fn authenticated(&self, headers: &HeaderMap) -> bool {
        let Some(hash) = self.cookie_hash(headers) else {
            return false;
        };
        let Ok(mut sessions) = self.sessions.lock() else {
            return false;
        };
        let now = Instant::now();
        sessions.active.retain(|_, expiry| *expiry > now);
        sessions.active.contains_key(&hash)
    }

    pub(super) fn same_origin(&self, headers: &HeaderMap) -> bool {
        if headers.get_all(header::ORIGIN).iter().count() != 1
            || headers.get_all(header::HOST).iter().count() != 1
        {
            return false;
        }
        let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let scheme = if self.secure { "https" } else { "http" };
        headers
            .get(header::ORIGIN)
            .is_some_and(|origin| origin.as_bytes() == format!("{scheme}://{host}").as_bytes())
    }

    fn set_cookie(&self, value: &str, max_age: u64) -> String {
        format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{}",
            self.cookie_name(),
            if self.secure { "; Secure" } else { "" }
        )
    }
}

pub(super) fn forbidden() -> ApiError {
    ApiError(
        StatusCode::FORBIDDEN,
        "invalid_origin",
        "A same-origin browser request is required",
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    password: String,
}

async fn login(
    State(state): State<Arc<Service>>,
    headers: HeaderMap,
    Json(login): Json<Login>,
) -> ApiResult<Response> {
    let web = state.web.as_ref().unwrap();
    if !web.same_origin(&headers) {
        return Err(forbidden());
    }
    let mut sessions = web.sessions.lock().map_err(|_| ApiError::internal())?;
    let now = Instant::now();
    sessions
        .failures
        .retain(|time| now.duration_since(*time) < LOGIN_WINDOW);
    if sessions.failures.len() >= MAX_FAILURES {
        let mut response = ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "login_rate_limit",
            "Too many failed login attempts; retry in one minute",
        )
        .into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "60".parse().unwrap());
        return Ok(response);
    }
    let hash: [u8; 32] = Sha256::digest(login.password.as_bytes()).into();
    if !bool::from(hash.ct_eq(&web.password)) {
        sessions.failures.push_back(now);
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_password",
            "The access password is incorrect",
        ));
    }
    sessions.active.retain(|_, expiry| *expiry > now);
    if sessions.active.len() >= MAX_SESSIONS {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_limit",
            "Too many active sessions",
        ));
    }
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    sessions.active.insert(
        Sha256::digest(token.as_bytes()).into(),
        now + SESSION_LIFETIME,
    );
    Ok((
        [(
            header::SET_COOKIE,
            web.set_cookie(&token, SESSION_LIFETIME.as_secs()),
        )],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

async fn logout(State(state): State<Arc<Service>>, headers: HeaderMap) -> ApiResult<Response> {
    let web = state.web.as_ref().unwrap();
    if !web.same_origin(&headers) {
        return Err(forbidden());
    }
    if let Some(hash) = web.cookie_hash(&headers) {
        web.sessions
            .lock()
            .map_err(|_| ApiError::internal())?
            .active
            .remove(&hash);
    }
    Ok((
        [(header::SET_COOKIE, web.set_cookie("", 0))],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

async fn session(State(state): State<Arc<Service>>, headers: HeaderMap) -> ApiResult<StatusCode> {
    if state.web.as_ref().unwrap().authenticated(&headers) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "session_expired",
            "Sign in to continue",
        ))
    }
}

async fn index(State(state): State<Arc<Service>>) -> Html<String> {
    Html(state.web.as_ref().unwrap().index.clone())
}

pub(super) fn router(state: &Arc<Service>) -> Router<Arc<Service>> {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route(
            "/auth/login",
            post(login).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session))
        .fallback_service(ServeDir::new(&state.web.as_ref().unwrap().directory))
}

pub(super) async fn gate(
    State(state): State<Arc<Service>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Only flat, unencoded build assets are public; encoded traversal must pass the gate.
    let asset = path.strip_prefix("/assets/").is_some_and(|name| {
        !name.is_empty()
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    });
    let public = asset
        || matches!(
            path,
            "/login.html" | "/brand/qingyu-mark.svg" | "/brand/qingyu-app.png"
        );
    let endpoint = matches!(
        path,
        "/auth/login" | "/auth/logout" | "/auth/session" | "/api"
    ) || path.starts_with("/api/");
    let mut response =
        if public || endpoint || state.web.as_ref().unwrap().authenticated(request.headers()) {
            next.run(request).await
        } else {
            Redirect::to("/login.html").into_response()
        };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::X_FRAME_OPTIONS, "SAMEORIGIN".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.append(
        header::CONTENT_SECURITY_POLICY,
        "frame-ancestors 'self'; base-uri 'self'; form-action 'self'"
            .parse()
            .unwrap(),
    );
    response
}

#[cfg(test)]
mod tests;
