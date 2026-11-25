//! The hub's entry points: HTTP for the browsers (a snapshot, the live event stream, settings, alerts, uptime, source management,
//! the login) and gRPC for the agents. The HTTP paths, the JSON and the status codes are those of the Go hub, so the web app does
//! not change.
//!
//! One controller module per resource (`auth_controller`, `live_controller`, `settings_controller`, `alerts_controller`,
//! `sources_controller`, and `agent_controller` for the agents' gRPC stream); this file is just the shared response/auth
//! plumbing the HTTP ones use, and the two routers (`routes` for the browsers, `agent_routes` for the agents) that wire
//! their handlers to paths.

mod access_controller;
mod agent_controller;
mod alerts_controller;
mod auth_controller;
mod comm_log_controller;
mod federation_controller;
mod federation_link_controller;
mod live_controller;
mod logs_controller;
mod registry_controller;
mod settings_controller;
mod sources_controller;
mod sso_controller;

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tracing::error;

use crate::app::AppState;
use crate::model::User;
use crate::policy::Cap;
use crate::services::{IIngestService, SharedCommLog, SharedUpgrades};
use crate::version::VERSION;

type Shared = Arc<AppState>;

pub use logs_controller::MAX_LOG_STREAMS;

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
        .route("/api/auth/sso/{id}/login", get(sso_controller::login))
        .route("/api/auth/sso/{id}/callback", get(sso_controller::callback))
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
            "/api/logs/{source}/{*target}",
            get(logs_controller::read_log),
        )
        .route(
            "/api/identity-providers",
            get(sso_controller::list).post(sso_controller::create),
        )
        .route("/api/identity-providers/test", post(sso_controller::test))
        .route(
            "/api/identity-providers/{id}",
            put(sso_controller::update).delete(sso_controller::delete),
        )
        .route(
            "/api/security",
            get(auth_controller::get_security).put(auth_controller::put_security),
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
        .route(
            "/api/users",
            get(access_controller::list_users).post(access_controller::create_user),
        )
        .route(
            "/api/users/{id}",
            patch(access_controller::update_user).delete(access_controller::delete_user),
        )
        .route(
            "/api/groups",
            get(access_controller::list_groups).post(access_controller::create_group),
        )
        .route(
            "/api/groups/{id}",
            patch(access_controller::update_group).delete(access_controller::delete_group),
        )
        .route(
            "/api/groups/{id}/members",
            put(access_controller::set_members),
        )
        .route(
            "/api/groups/{id}/policies",
            put(access_controller::set_group_policies),
        )
        .route(
            "/api/policies",
            get(access_controller::list_policies).post(access_controller::create_policy),
        )
        .route("/api/policies/check", get(access_controller::check))
        .route(
            "/api/policies/{id}",
            patch(access_controller::update_policy).delete(access_controller::delete_policy),
        )
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

/// The master's pages for its datacenters, and the tunnel that carries the dashboard's admin calls to them. Merged onto `routes()`'s router in
/// `app::Hub::router`.
pub fn federation_routes(state: Shared) -> Router {
    federation_controller::routes(state)
}

/// The gRPC side, over gRPC rather than HTTP: the agents' stream and the link a master opens to this hub when it is a member. Both services
/// share one router, because a router can have only one fallback.
pub fn agent_routes(
    state: Shared,
    ingest: Arc<dyn IIngestService>,
    upgrades: SharedUpgrades,
    comm_log: SharedCommLog,
) -> Router {
    let routes = agent_controller::routes(ingest, upgrades, state.logs.clone(), comm_log);
    federation_link_controller::add(routes, state).into_axum_router()
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

/// Set only by this process on the requests its federation link makes to its own router: a master's admin request, run here as if an admin had
/// sent it. The value is random per process and never leaves it, so nothing from the network can carry it.
const LINK_HEADER: &str = "x-hermes-link";

/// Is this request one the link of this very process made on behalf of the master that manages this hub?
fn from_link(state: &Shared, headers: &HeaderMap) -> bool {
    headers
        .get(LINK_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| state.federation.is_link_secret(v))
}

/// The master that manages this hub, standing in for the admin.
fn link_user() -> User {
    User {
        id: 0,
        username: "master".into(),
        password_hash: String::new(),
        role: "admin".into(),
        created: 0,
        email: String::new(),
        disabled: false,
        break_glass: false,
        idp: String::new(),
        external_id: String::new(),
    }
}

/// A hub that another one manages has no logins: whatever session cookie a browser holds is worth nothing.
fn session_user(state: &Shared, headers: &HeaderMap) -> Option<User> {
    if state.federation.is_locked() {
        return None;
    }
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

/// What a request is, to the policies: the path without `/api/` (`/api/sources/s1` is `sources/s1`) and the capability its method asks for.
fn policy_request(parts: &Parts) -> Option<(Cap, String)> {
    let cap = Cap::of_method(parts.method.as_str())?;
    let path = parts.uri.path();
    let path = path.strip_prefix("/api/").unwrap_or(path);
    Some((cap, crate::policy::clean_path(path)))
}

fn forbidden(cap: Cap, path: &str) -> Response {
    fail(
        StatusCode::FORBIDDEN,
        &format!("not allowed: {} on {path}", cap.name()),
    )
}

/// Someone who is logged in and whose policies allow this very call (the method and the path); a call that changes something must also
/// carry the CSRF header. A master hub's own link is allowed everything on the hub it manages.
struct Admin(User);

impl FromRequestParts<Shared> for Admin {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        if from_link(state, &parts.headers) {
            return Ok(Admin(link_user()));
        }
        let user = session_user(state, &parts.headers).ok_or_else(login_required)?;
        let read_only = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if !read_only && !csrf_ok(&parts.headers) {
            return Err(csrf_failed());
        }
        let Some((cap, path)) = policy_request(parts) else {
            return Err(fail(StatusCode::METHOD_NOT_ALLOWED, "method not allowed"));
        };
        if !state.authz.allowed(&user, cap, &path) {
            return Err(forbidden(cap, &path));
        }
        Ok(Admin(user))
    }
}

/// Anyone who is logged in, whatever their policies: the calls about their own session (their own password).
struct Login(User);

impl FromRequestParts<Shared> for Login {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        let user = session_user(state, &parts.headers).ok_or_else(login_required)?;
        if !csrf_ok(&parts.headers) {
            return Err(csrf_failed());
        }
        Ok(Login(user))
    }
}

/// Which clusters (sources) a person may see: a person who may read `clusters/<source id>` sees that cluster, its map, its alerts and its uptime, and
/// nothing of the ones they may not. The wallboard that is public, and a master hub's own link, see everything. A node, an edge or an alert belongs to the
/// source its id starts with (`<source id>:...`).
pub(crate) struct Sight {
    everything: bool,
    user: Option<User>,
    authz: Arc<dyn crate::services::IAuthz>,
    /// Answers for a few seconds: a stream asks about the same source for every event.
    memo: std::sync::Mutex<(std::time::Instant, std::collections::HashMap<String, bool>)>,
}

impl Sight {
    fn new_memo() -> std::sync::Mutex<(std::time::Instant, std::collections::HashMap<String, bool>)>
    {
        std::sync::Mutex::new((std::time::Instant::now(), std::collections::HashMap::new()))
    }

    fn all(authz: Arc<dyn crate::services::IAuthz>) -> Arc<Self> {
        Arc::new(Self {
            everything: true,
            user: None,
            authz,
            memo: Self::new_memo(),
        })
    }

    fn of(user: User, authz: Arc<dyn crate::services::IAuthz>) -> Arc<Self> {
        Arc::new(Self {
            everything: false,
            user: Some(user),
            authz,
            memo: Self::new_memo(),
        })
    }

    /// Sees everything: nothing needs filtering.
    pub(crate) fn everything(&self) -> bool {
        self.everything
    }

    pub(crate) fn sees(&self, source: &str) -> bool {
        if self.everything {
            return true;
        }
        let Some(user) = &self.user else { return false };
        let mut memo = self
            .memo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if memo.0.elapsed() > std::time::Duration::from_secs(5) {
            *memo = (std::time::Instant::now(), std::collections::HashMap::new());
        }
        if let Some(known) = memo.1.get(source) {
            return *known;
        }
        let yes = self
            .authz
            .allowed(user, Cap::Read, &format!("clusters/{source}"));
        memo.1.insert(source.to_string(), yes);
        yes
    }

    /// A node, an edge or an alert, by its id: it starts with the id of its source.
    pub(crate) fn sees_node(&self, id: &str) -> bool {
        self.everything || self.sees(id.split(':').next().unwrap_or(id))
    }

    /// May read this path (not a cluster): for what only some may see of the clusters they do.
    pub(crate) fn may(&self, path: &str) -> bool {
        self.everything
            || self
                .user
                .as_ref()
                .is_some_and(|u| self.authz.allowed(u, Cap::Read, path))
    }
}

/// Who is looking at read-only data, and what of the clusters they may see. A route is open to whoever may read its path, and to whoever may see some
/// cluster (a team that has been given its own cluster reads the map without a rule for `snapshot`); a `deny` on the path closes it for everyone.
// the error is the response the extractor rejects with, as it is
#[allow(clippy::result_large_err)]
fn sight_of(parts: &Parts, state: &Shared, anonymous: bool) -> Result<Arc<Sight>, Response> {
    if from_link(state, &parts.headers) {
        return Ok(Sight::all(state.authz.clone()));
    }
    let session = session_user(state, &parts.headers);
    // The wallboard (`?view=tv`) shows everything when the admin made it public: it is the one screen that is meant for the room, not for the person.
    // Somebody who is logged in and looks at the admin panel is shown what their policies let them, whether the wallboard is public or not. Somebody
    // who is not logged in sees the public wallboard's data.
    let wallboard = parts
        .uri
        .query()
        .is_some_and(|q| q.split('&').any(|kv| kv == "view=tv"));
    let public = anonymous && state.auth.public_view() && !state.federation.is_locked();
    if public && (session.is_none() || wallboard) {
        return Ok(Sight::all(state.authz.clone()));
    }
    let user = session.ok_or_else(login_required)?;
    // a wallboard that is not public still shows everything to who may read `wallboard`: a group that is given it sees every cluster on the TV, and
    // only its own in the admin panel
    if wallboard && state.authz.allowed(&user, Cap::Read, "wallboard") {
        return Ok(Sight::all(state.authz.clone()));
    }
    let (cap, path) = policy_request(parts).unwrap_or((Cap::Read, String::new()));
    let sight = Sight::of(user.clone(), state.authz.clone());
    match state.authz.decide(&user, cap, &path) {
        crate::policy::Decision::Allowed { .. } => Ok(sight),
        crate::policy::Decision::Denied { .. } => Err(forbidden(cap, &path)),
        crate::policy::Decision::NoRule => {
            let local = state.db.sources.list_sources().unwrap_or_default();
            let sees_one = local.iter().any(|s| sight.sees(&s.id))
                || state
                    .links
                    .source_views()
                    .iter()
                    .any(|v| v["id"].as_str().is_some_and(|id| sight.sees(id)));
            if sees_one {
                Ok(sight)
            } else {
                Err(forbidden(cap, &path))
            }
        }
    }
}

/// For read-only data: a logged-in person (what they see of the clusters is theirs to see), or anyone when the wallboard is public.
struct Viewer(Arc<Sight>);

impl FromRequestParts<Shared> for Viewer {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        sight_of(parts, state, true).map(Viewer)
    }
}

/// The same for what a public wallboard does not get: somebody who is logged in.
struct Member(Arc<Sight>);

impl FromRequestParts<Shared> for Member {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &Shared) -> Result<Self, Response> {
        sight_of(parts, state, false).map(Member)
    }
}
