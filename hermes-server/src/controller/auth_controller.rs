//! The login flow: first-run setup, sign in/out, password changes, and the public-view toggle.

use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;

use super::{
    Admin, COOKIE_NAME, Login, Shared, cookie, csrf_failed, csrf_ok, fail, internal, json_response,
    ok, parse,
};
use crate::app::Settings;
use crate::model::AuthStatus;
use crate::model::SessionPolicy;
use crate::services::AuthError;

/// `tls_enabled` is the hub's own TLS (`HUB_TLS_CERT`/`HUB_TLS_KEY`); the header covers the other case, a proxy in front that
/// terminates TLS itself and forwards plain HTTP.
pub(super) fn session_cookie(
    headers: &HeaderMap,
    settings: &Settings,
    max_age_secs: i64,
    token: &str,
) -> (HeaderName, HeaderValue) {
    let secure = settings.tls_enabled
        || headers
            .get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("https"));
    let value = format!(
        "{COOKIE_NAME}={token}; Path={}; HttpOnly; SameSite=Strict; Max-Age={}{}",
        cookie_path(settings),
        max_age_secs,
        if secure { "; Secure" } else { "" }
    );
    (
        header::SET_COOKIE,
        HeaderValue::from_str(&value).expect("a session token is plain ASCII"),
    )
}

/// The cookie belongs to the hub's own path, so a hub under `/hermes` does not send it to whatever else shares the host.
fn cookie_path(settings: &Settings) -> &str {
    if settings.base_path.is_empty() {
        "/"
    } else {
        &settings.base_path
    }
}

fn with_cookie(
    headers: &HeaderMap,
    settings: &Settings,
    max_age_secs: i64,
    token: &str,
    mut response: Response,
) -> Response {
    let (name, value) = session_cookie(headers, settings, max_age_secs, token);
    response.headers_mut().insert(name, value);
    response
}

fn auth_error(e: AuthError) -> Response {
    let code = match e {
        AuthError::BadCredentials => StatusCode::UNAUTHORIZED,
        AuthError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        AuthError::SetupDone => StatusCode::CONFLICT,
        AuthError::BadUsername | AuthError::WeakPassword | AuthError::BadSessionPolicy => {
            StatusCode::BAD_REQUEST
        }
        AuthError::Internal(ref inner) => return internal(inner),
    };
    fail(code, &e.to_string())
}

/// Hashing takes a while and a lot of memory: off the async threads.
#[allow(clippy::result_large_err)]
pub(super) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Response> {
    tokio::task::spawn_blocking(work).await.map_err(internal)
}

/// The answer of a hub that another one manages, to whoever tries to log in to it.
fn managed(st: &Shared) -> Option<Response> {
    st.federation.is_locked().then(|| {
        fail(
            StatusCode::FORBIDDEN,
            "this hub is managed by another hub: log in there",
        )
    })
}

pub(super) async fn auth_status(State(st): State<Shared>, headers: HeaderMap) -> Response {
    let need = match st.auth.needs_setup() {
        Ok(need) => need,
        Err(e) => return auth_error(e),
    };
    let user = super::session_user(&st, &headers);
    let status = AuthStatus {
        // a hub that is managed by another one never asks for an admin: it has no logins
        setup_required: need && !st.federation.is_locked(),
        authenticated: user.is_some(),
        public_view: st.auth.public_view() && !st.federation.is_locked(),
        grants: user.as_ref().map(|u| st.authz.grants(u)),
        sso: user
            .is_none()
            .then(|| st.sso.offered())
            .filter(|p| !p.is_empty()),
        username: user.map(|u| u.username),
        member: st.federation.member_view(crate::database::now_ms()),
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
    settings: &Settings,
    max_age_secs: i64,
    ok_body: Response,
    work: impl FnOnce() -> Result<String, AuthError> + Send + 'static,
) -> Response {
    match blocking(work).await {
        Ok(Ok(token)) => with_cookie(headers, settings, max_age_secs, &token, ok_body),
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
    if let Some(r) = managed(&st) {
        return r;
    }
    let c: Credentials = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let auth = st.auth.clone();
    let ok_body = json_response(StatusCode::CREATED, json!({"ok": true}));
    login_response(
        &headers,
        &st.settings,
        st.auth.session_policy().max_ms() / 1000,
        ok_body,
        move || auth.setup(&c.username, &c.password, c.public_view),
    )
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
    if let Some(r) = managed(&st) {
        return r;
    }
    let c: Credentials = match parse(&body) {
        Ok(c) => c,
        Err(r) => return r,
    };
    let (auth, ip) = (st.auth.clone(), peer.ip().to_string());
    login_response(
        &headers,
        &st.settings,
        st.auth.session_policy().max_ms() / 1000,
        ok(),
        move || auth.login(&c.username, &c.password, &ip),
    )
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
        HeaderValue::from_str(&format!(
            "{COOKIE_NAME}=; Path={}; HttpOnly; Max-Age=0",
            cookie_path(&st.settings)
        ))
        .expect("the base path is plain ASCII"),
    );
    response
}

pub(super) async fn auth_password(
    State(st): State<Shared>,
    Login(user): Login,
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
    login_response(
        &headers,
        &st.settings,
        st.auth.session_policy().max_ms() / 1000,
        ok(),
        move || auth.change_password(user.id, &change.current, &change.next),
    )
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

/// `GET /api/security`.
pub(super) async fn get_security(State(st): State<Shared>, _: Admin) -> Response {
    json_response(StatusCode::OK, st.auth.session_policy())
}

/// `PUT /api/security`: how long a login lasts. It counts for the logins that exist as well: the next request of each is judged by it.
pub(super) async fn put_security(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let policy: SessionPolicy = match parse(&body) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match st.auth.set_session_policy(policy) {
        Ok(()) => json_response(StatusCode::OK, policy),
        Err(e) => auth_error(e),
    }
}
