//! Single sign-on: the two addresses a person passes through (`/api/auth/sso/{id}/login` sends them to the provider, `.../callback` is where the
//! provider sends them back), and the admin's pages for the providers (`/api/identity-providers`).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use super::auth_controller::session_cookie;
use super::{Admin, Shared, cookie, fail, internal, json_response, ok, parse};
use crate::model::{ProviderInput, ProviderTest};
use crate::services::SsoError;

const SSO_COOKIE: &str = "hermes_sso";

fn error(e: SsoError) -> Response {
    let code = match e {
        SsoError::Invalid(_) | SsoError::BadState => StatusCode::BAD_REQUEST,
        SsoError::NotFound => StatusCode::NOT_FOUND,
        SsoError::Provider(_) => StatusCode::BAD_GATEWAY,
        SsoError::Refused(_) => StatusCode::FORBIDDEN,
        SsoError::Internal(ref inner) => return internal(inner),
    };
    fail(code, &e.to_string())
}

/// The address a provider sends the person back to: where this hub is reached (`HUB_PUBLIC_URL`, or what the request says, behind a proxy
/// the forwarded headers) with the base path and the callback path. It is registered at the provider, which refuses any other.
fn callback_url(st: &Shared, headers: &HeaderMap, id: &str) -> String {
    let origin = st.settings.public_url.clone().unwrap_or_else(|| {
        let get = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
        };
        let proto = get("x-forwarded-proto").unwrap_or_else(|| {
            if st.settings.tls_enabled {
                "https".into()
            } else {
                "http".into()
            }
        });
        let host = get("x-forwarded-host")
            .or_else(|| get("host"))
            .unwrap_or_else(|| "localhost".into());
        format!("{proto}://{host}")
    });
    format!(
        "{origin}{}/api/auth/sso/{id}/callback",
        st.settings.base_path
    )
}

fn redirect(to: &str, cookies: &[String]) -> Response {
    let mut response = (StatusCode::FOUND, [(header::LOCATION, to.to_string())]).into_response();
    for c in cookies {
        if let Ok(v) = HeaderValue::from_str(c) {
            response.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    response
}

/// Back to the page with what went wrong (the login screen shows it): nothing about the provider's answer is put in the address but our own words.
fn failed(st: &Shared, message: &str, cookies: &[String]) -> Response {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("sso_error", message)
        .finish();
    redirect(
        &format!("{}/?{query}#/admin", st.settings.base_path),
        cookies,
    )
}

fn sso_cookie(st: &Shared, value: &str, max_age: u32) -> String {
    let path = if st.settings.base_path.is_empty() {
        "/"
    } else {
        &st.settings.base_path
    };
    // Lax: the provider's redirect back to us is a navigation from another site, which Strict would not send the cookie with
    let secure = if st.settings.tls_enabled {
        "; Secure"
    } else {
        ""
    };
    format!("{SSO_COOKIE}={value}; Path={path}; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
}

pub(super) async fn login(
    State(st): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if st.federation.is_locked() {
        return failed(
            &st,
            "this hub is managed by another hub: sign in there",
            &[],
        );
    }
    match st.sso.begin(&id, &callback_url(&st, &headers, &id)).await {
        Ok(begin) => redirect(&begin.url, &[sso_cookie(&st, &begin.state, 600)]),
        Err(e) => failed(&st, &e.to_string(), &[]),
    }
}

#[derive(Deserialize)]
pub(super) struct Back {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(super) async fn callback(
    State(st): State<Shared>,
    Path(id): Path<String>,
    Query(back): Query<Back>,
    headers: HeaderMap,
) -> Response {
    let clear = sso_cookie(&st, "", 0);
    if let Some(e) = &back.error {
        let why = back.error_description.as_deref().unwrap_or(e);
        return failed(
            &st,
            &format!(
                "the provider did not sign you in: {}",
                why.chars().take(200).collect::<String>()
            ),
            &[clear],
        );
    }
    let (Some(code), Some(state)) = (back.code.as_deref(), back.state.as_deref()) else {
        return failed(&st, "the provider sent nothing to sign in with", &[clear]);
    };
    // the browser that came back is the one that left: otherwise someone could hand their own sign-in to somebody else
    if cookie(&headers, SSO_COOKIE).as_deref() != Some(state) {
        return failed(&st, &SsoError::BadState.to_string(), &[clear]);
    }
    let user = match st.sso.finish(&id, code, state).await {
        Ok(u) => u,
        Err(e) => {
            if let SsoError::Internal(inner) = &e {
                tracing::error!("single sign-on: {inner:#}");
            }
            return failed(&st, &e.to_string(), &[clear]);
        }
    };
    let token = match st.auth.session_for(user.id) {
        Ok(t) => t,
        Err(_) => return failed(&st, "this account cannot sign in", &[clear]),
    };
    let (_, value) = session_cookie(
        &headers,
        &st.settings,
        st.auth.session_policy().max_ms() / 1000,
        &token,
    );
    let session = value.to_str().unwrap_or_default().to_string();
    redirect(
        &format!("{}/#/admin/dashboard", st.settings.base_path),
        &[clear, session],
    )
}

// ---- the admin's side ----------------------------------------------------------------------------------------------

pub(super) async fn list(State(st): State<Shared>, _: Admin, headers: HeaderMap) -> Response {
    match st.sso.views(&|id| callback_url(&st, &headers, id)) {
        Ok(v) => json_response(StatusCode::OK, v),
        Err(e) => error(e),
    }
}

pub(super) async fn create(
    State(st): State<Shared>,
    _: Admin,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let input: ProviderInput = match parse(&body) {
        Ok(i) => i,
        Err(r) => return r,
    };
    match st.sso.create(&input, &|id| callback_url(&st, &headers, id)) {
        Ok(v) => json_response(StatusCode::CREATED, v),
        Err(e) => error(e),
    }
}

pub(super) async fn update(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let input: ProviderInput = match parse(&body) {
        Ok(i) => i,
        Err(r) => return r,
    };
    match st
        .sso
        .update(&id, &input, &|id| callback_url(&st, &headers, id))
    {
        Ok(v) => json_response(StatusCode::OK, v),
        Err(e) => error(e),
    }
}

pub(super) async fn delete(State(st): State<Shared>, _: Admin, Path(id): Path<String>) -> Response {
    match st.sso.delete(&id) {
        Ok(()) => ok(),
        Err(e) => error(e),
    }
}

pub(super) async fn test(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Ask {
        issuer: String,
    }
    let ask: Ask = match parse(&body) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let answer = match st.sso.test(&ask.issuer).await {
        Ok(message) => ProviderTest { ok: true, message },
        Err(e @ (SsoError::Invalid(_) | SsoError::Provider(_))) => ProviderTest {
            ok: false,
            message: e.to_string(),
        },
        Err(e) => return error(e),
    };
    json_response(StatusCode::OK, answer)
}
