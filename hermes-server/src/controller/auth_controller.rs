//! The login flow: first-run setup, sign in/out, password changes, and the public-view toggle.

use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use super::{
    Admin, COOKIE_NAME, Shared, cookie, csrf_failed, csrf_ok, fail, internal, json_response, ok,
    parse,
};
use crate::model::AuthStatus;
use crate::services::{AuthError, SESSION_TTL_MS};

/// `tls_enabled` is the hub's own TLS (`HUB_TLS_CERT`/`HUB_TLS_KEY`); the header covers the other case, a proxy in front that
/// terminates TLS itself and forwards plain HTTP.
fn session_cookie(
    headers: &HeaderMap,
    tls_enabled: bool,
    token: &str,
) -> (HeaderName, HeaderValue) {
    let secure = tls_enabled
        || headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("https"));
    let value = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        SESSION_TTL_MS / 1000,
        if secure { "; Secure" } else { "" }
    );
    (
        header::SET_COOKIE,
        HeaderValue::from_str(&value).expect("a session token is plain ASCII"),
    )
}

fn with_cookie(
    headers: &HeaderMap,
    tls_enabled: bool,
    token: &str,
    mut response: Response,
) -> Response {
    let (name, value) = session_cookie(headers, tls_enabled, token);
    response.headers_mut().insert(name, value);
    response
}

fn auth_error(e: AuthError) -> Response {
    let code = match e {
        AuthError::BadCredentials => StatusCode::UNAUTHORIZED,
        AuthError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        AuthError::SetupDone => StatusCode::CONFLICT,
        AuthError::BadUsername | AuthError::WeakPassword => StatusCode::BAD_REQUEST,
        AuthError::Internal(ref inner) => return internal(inner),
    };
    fail(code, &e.to_string())
}

/// Hashing takes a while and a lot of memory: off the async threads.
#[allow(clippy::result_large_err)]
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Response> {
    tokio::task::spawn_blocking(work).await.map_err(internal)
}

pub(super) async fn auth_status(State(st): State<Shared>, headers: HeaderMap) -> Response {
    let need = match st.auth.needs_setup() {
        Ok(need) => need,
        Err(e) => return auth_error(e),
    };
    let user = super::session_user(&st, &headers);
    let status = AuthStatus {
        setup_required: need,
        authenticated: user.is_some(),
        public_view: st.auth.public_view(),
        username: user.map(|u| u.username),
    };
    json_response(StatusCode::OK, status)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Credentials {
    username: String,
    password: String,
    #[serde(rename = "publicView")]
    public_view: bool,
}

/// Runs an auth operation off the async threads and, once it succeeds, attaches the session cookie to `ok_body`. The three outcomes
/// (`login`/`setup`/`change_password` can fail with an `AuthError`, or the blocking task itself can panic) all become one `Response`.
async fn login_response(
    headers: &HeaderMap,
    tls_enabled: bool,
    ok_body: Response,
    work: impl FnOnce() -> Result<String, AuthError> + Send + 'static,
) -> Response {
    match blocking(work).await {
        Ok(Ok(token)) => with_cookie(headers, tls_enabled, &token, ok_body),
        Ok(Err(e)) => auth_error(e),
        Err(r) => r,
    }
}

pub(super) async fn auth_setup(
    State(st): State<Shared>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !csrf_ok(&headers) {
        return csrf_failed();
    }
    let c: Credentials = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let auth = st.auth.clone();
    let ok_body = json_response(StatusCode::CREATED, json!({"ok": true}));
    login_response(&headers, st.settings.tls_enabled, ok_body, move || {
        auth.setup(&c.username, &c.password, c.public_view)
    })
    .await
}

pub(super) async fn auth_login(
    State(st): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !csrf_ok(&headers) {
        return csrf_failed();
    }
    let c: Credentials = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let (auth, ip) = (st.auth.clone(), peer.ip().to_string());
    login_response(&headers, st.settings.tls_enabled, ok(), move || {
        auth.login(&c.username, &c.password, &ip)
    })
    .await
}

pub(super) async fn auth_logout(State(st): State<Shared>, headers: HeaderMap) -> Response {
    if !csrf_ok(&headers) {
        return csrf_failed();
    }
    if let Some(token) = cookie(&headers, COOKIE_NAME) {
        st.auth.logout(&token);
    }
    let mut response = ok();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("hermes_session=; Path=/; HttpOnly; Max-Age=0"),
    );
    response
}

pub(super) async fn auth_password(
    State(st): State<Shared>,
    Admin(user): Admin,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Change {
        current: String,
        next: String,
    }
    let change: Change = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let auth = st.auth.clone();
    login_response(&headers, st.settings.tls_enabled, ok(), move || {
        auth.change_password(user.id, &change.current, &change.next)
    })
    .await
}

pub(super) async fn auth_public_view(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Body {
        enabled: bool,
    }
    match parse::<Body>(&body) {
        Ok(b) => st
            .auth
            .set_public_view(b.enabled)
            .map_or_else(auth_error, |()| ok()),
        Err(r) => r,
    }
}
