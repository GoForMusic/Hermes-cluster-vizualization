//! The hub's entry points: HTTP for the browsers (a snapshot, the live event stream, settings, alerts, uptime, source management,
//! the login) and gRPC for the agents. The HTTP paths, the JSON and the status codes are those of the Go hub, so the web app does
//! not change.
//!
//! One controller module per resource (`auth_controller`, `live_controller`, `settings_controller`, `alerts_controller`,
//! `sources_controller`, and `agent_controller` for the agents' gRPC stream); this file is just the shared response/auth
//! plumbing the HTTP ones use, and the two routers (`routes` for the browsers, `agent_routes` for the agents) that wire
//! their handlers to paths.

mod agent_controller;
mod alerts_controller;
mod auth_controller;
mod comm_log_controller;
mod live_controller;
mod registry_controller;
mod settings_controller;
mod sources_controller;

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::error;

use crate::app::AppState;
use crate::model::User;
use crate::services::{IIngestService, SharedCommLog, SharedUpgrades};
use crate::version::VERSION;

type Shared = Arc<AppState>;

const COOKIE_NAME: &str = "hermes_session";
/// A custom header cannot be sent cross-origin without a CORS preflight: its presence is the proof that the page is ours.
const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "hermes";

const SMALL_BODY: usize = 1 << 20;
const AUTH_BODY: usize = 1 << 16;

pub fn routes(state: Shared) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"ok": true, "version": VERSION})) }),
        )
        // public: the login flow, and the agents (which carry their own token)
        .route("/api/auth/status", get(auth_controller::auth_status))
        .route(
            "/api/auth/setup",
            post(auth_controller::auth_setup).layer(DefaultBodyLimit::max(AUTH_BODY)),
        )
        .route(
            "/api/auth/login",
            post(auth_controller::auth_login).layer(DefaultBodyLimit::max(AUTH_BODY)),
        )
        .route("/api/auth/logout", post(auth_controller::auth_logout))
        .route(
            "/api/auth/password",
            post(auth_controller::auth_password).layer(DefaultBodyLimit::max(AUTH_BODY)),
        )
        .route(
            "/api/auth/public-view",
            put(auth_controller::auth_public_view).layer(DefaultBodyLimit::max(AUTH_BODY)),
        )
        // read-only data: an admin, or anyone while the wallboard is public
        .route("/api/info", get(live_controller::info))
        .route("/api/snapshot", get(live_controller::snapshot))
        .route("/api/stream", get(live_controller::stream))
        .route(
            "/api/settings",
            get(settings_controller::get_settings).put(settings_controller::put_settings),
        )
        .route(
            "/api/registry",
            get(registry_controller::get_registry)
                .put(registry_controller::put_registry)
                .delete(registry_controller::clear_registry),
        )
        .route(
            "/api/registry/test",
            post(registry_controller::test_registry),
        )
        .route(
            "/api/registry/versions",
            get(registry_controller::registry_versions),
        )
        .route(
            "/api/comm-log",
            get(comm_log_controller::list_log)
                .put(comm_log_controller::switch_log)
                .delete(comm_log_controller::clear_log),
        )
        .route("/api/comm-log/{id}", get(comm_log_controller::get_entry))
        .route("/api/alerts", get(alerts_controller::list_alerts))
        .route("/api/uptime", get(alerts_controller::uptime))
        // everything below changes something or shows source details: admin only
        .route("/api/alerts/{id}/ack", post(alerts_controller::ack_alert))
        .route(
            "/api/sources",
            get(sources_controller::list_sources).post(sources_controller::add_source),
        )
        .route(
            "/api/sources/{id}",
            delete(sources_controller::remove_source).patch(sources_controller::update_source),
        )
        .route(
            "/api/sources/{id}/upgrade",
            post(sources_controller::upgrade_source),
        )
        .layer(DefaultBodyLimit::max(SMALL_BODY))
        .with_state(state)
}

/// The agents' side, over gRPC rather than HTTP — merged onto `routes()`'s router in `app::Hub::router`.
pub fn agent_routes(
    ingest: Arc<dyn IIngestService>,
    upgrades: SharedUpgrades,
    comm_log: SharedCommLog,
) -> Router {
    agent_controller::routes(ingest, upgrades, comm_log)
}

// ---- answers ---------------------------------------------------------------------------------------------------

fn json_response(code: StatusCode, body: impl Serialize) -> Response {
    (code, Json(body)).into_response()
}

fn ok() -> Response {
    json_response(StatusCode::OK, json!({"ok": true}))
}

fn fail(code: StatusCode, message: &str) -> Response {
    json_response(code, json!({"error": message}))
}

fn internal(e: impl std::fmt::Display) -> Response {
    error!("{e:#}"); // `{:#}` shows an anyhow error's whole context chain, not just its top line
    fail(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

fn login_required() -> Response {
    json_response(
        StatusCode::UNAUTHORIZED,
        json!({"error": "login required", "auth": true}),
    )
}

// the error is the response the handler returns as it is: boxing it would only add a step
#[allow(clippy::result_large_err)]
fn parse<T: DeserializeOwned>(body: &Bytes) -> Result<T, Response> {
    serde_json::from_slice(body).map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid request"))
}

// ---- who is asking ----------------------------------------------------------------------------------------------

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| {
            let (k, v) = kv.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
}

fn session_user(state: &Shared, headers: &HeaderMap) -> Option<User> {
    state.auth.authenticate(&cookie(headers, COOKIE_NAME)?)
}

fn csrf_ok(headers: &HeaderMap) -> bool {
    headers
        .get(CSRF_HEADER)
        .is_some_and(|v| v.as_bytes() == CSRF_VALUE.as_bytes())
}

fn csrf_failed() -> Response {
    fail(StatusCode::FORBIDDEN, "missing X-Requested-With header")
}

/// A logged-in admin; a call that changes something must also carry the CSRF header.
struct Admin(User);

impl FromRequestParts<Shared> for Admin {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        let user = session_user(state, &parts.headers).ok_or_else(login_required)?;
        let read_only = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if !read_only && !csrf_ok(&parts.headers) {
            return Err(csrf_failed());
        }
        Ok(Admin(user))
    }
}

/// For read-only data: a logged-in admin, or anyone when the wallboard is public.
struct Viewer;

impl FromRequestParts<Shared> for Viewer {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        if session_user(state, &parts.headers).is_some() || state.auth.public_view() {
            Ok(Viewer)
        } else {
            Err(login_required())
        }
    }
}
