//! Federation in the browser: a member's pairing step, a master's datacenters, and the tunnel that carries the dashboard's admin calls to a
//! datacenter.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, patch, post};
use serde::Deserialize;
use serde_json::json;

use super::federation_link_controller::link_allowed;
use super::{
    Admin, SMALL_BODY, Shared, csrf_failed, csrf_ok, fail, from_link, internal, json_response,
    login_required, ok, parse, session_user,
};
use crate::database::now_ms;
use crate::model::{Datacenter, FederationView};
use crate::services::{FedError, LinkError};

pub(super) fn routes(state: Shared) -> Router {
    Router::new()
        .route(
            "/api/federation",
            get(get_federation).patch(rename_this_hub),
        )
        .route(
            "/api/federation/member",
            post(enable_member)
                .patch(patch_member)
                .delete(disable_member),
        )
        .route("/api/federation/member/code", post(new_code))
        .route("/api/federation/datacenters", post(add_datacenter))
        .route(
            "/api/federation/datacenters/{id}",
            patch(update_datacenter).delete(remove_datacenter),
        )
        .route("/api/dc/{id}/{*path}", any(tunnel))
        .layer(DefaultBodyLimit::max(SMALL_BODY))
        .with_state(state)
}

fn fed_error(e: FedError) -> Response {
    let code = match e {
        FedError::NotWaiting | FedError::Locked => StatusCode::CONFLICT,
        FedError::Expired | FedError::BadCode | FedError::Invalid(_) => StatusCode::BAD_REQUEST,
        FedError::Internal(ref inner) => return internal(inner),
    };
    fail(code, &e.to_string())
}

fn view(st: &Shared) -> FederationView {
    FederationView {
        name: st.federation.name(),
        member: st.federation.member_view(now_ms()),
        datacenters: st.links.views(),
    }
}

/// Who may set this hub up to be managed: an admin, or anyone while it has no admin yet (the same exposure as its first-run setup, and
/// a master still needs the code to take it over).
#[allow(clippy::result_large_err)]
fn may_configure(st: &Shared, headers: &HeaderMap) -> Result<(), Response> {
    if !csrf_ok(headers) {
        return Err(csrf_failed());
    }
    let fresh = st.auth.needs_setup().unwrap_or(false) && !st.federation.is_locked();
    if from_link(st, headers) || session_user(st, headers).is_some() || fresh {
        Ok(())
    } else {
        Err(login_required())
    }
}

pub(super) async fn get_federation(State(st): State<Shared>, _: Admin) -> Response {
    json_response(StatusCode::OK, view(&st))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct NameRequest {
    name: String,
}

/// What this hub is called next to its datacenters.
async fn rename_this_hub(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let req: NameRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match st.federation.set_name(&req.name) {
        Ok(()) => json_response(StatusCode::OK, view(&st)),
        Err(e) => fed_error(e),
    }
}

// ---- this hub is managed by another one --------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct MemberRequest {
    agent_address: String,
}

fn code_response(st: &Shared, code: &str, expires: i64) -> Response {
    json_response(
        StatusCode::OK,
        json!({"code": code, "expiresAt": expires, "member": st.federation.member_view(now_ms())}),
    )
}

/// "This hub will be managed by another one": it shows a pairing code and waits for the master to come with it.
async fn enable_member(State(st): State<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    if let Err(r) = may_configure(&st, &headers) {
        return r;
    }
    let req: MemberRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if !req.agent_address.is_empty()
        && !(req.agent_address.starts_with("http://") || req.agent_address.starts_with("https://"))
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "the address starts with http:// or https://",
        );
    }
    match st.federation.enable_member(&req.agent_address, now_ms()) {
        Ok((code, expires)) => code_response(&st, &code, expires),
        Err(e) => fed_error(e),
    }
}

async fn new_code(State(st): State<Shared>, headers: HeaderMap) -> Response {
    if let Err(r) = may_configure(&st, &headers) {
        return r;
    }
    match st.federation.new_code(now_ms()) {
        Ok((code, expires)) => code_response(&st, &code, expires),
        Err(e) => fed_error(e),
    }
}

/// Nobody came: back to being a hub on its own. A hub that was paired is released from its master, not from here.
async fn disable_member(State(st): State<Shared>, headers: HeaderMap) -> Response {
    if let Err(r) = may_configure(&st, &headers) {
        return r;
    }
    match st.federation.disable_member() {
        Ok(()) => ok(),
        Err(e) => fed_error(e),
    }
}

/// Where the agents of this hub find it. Also reachable from the master, through the tunnel.
async fn patch_member(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let req: MemberRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match st.federation.set_agent_address(&req.agent_address) {
        Ok(()) => json_response(StatusCode::OK, view(&st)),
        Err(e) => fed_error(e),
    }
}

// ---- this hub controls datacenters ----------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct DatacenterRequest {
    name: String,
    address: String,
    code: String,
}

fn rand_hex(bytes: usize) -> String {
    hex::encode((0..bytes).map(|_| rand::random::<u8>()).collect::<Vec<_>>())
}

fn clean_address(address: &str) -> Option<String> {
    let a = address.trim().trim_end_matches('/');
    ((a.starts_with("http://") || a.starts_with("https://")) && a.len() > 8).then(|| a.to_string())
}

/// Adds a datacenter: shows the member the pairing code it displayed, keeps the token it answers with, and starts the link.
async fn add_datacenter(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let req: DatacenterRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let name = req.name.trim().to_string();
    if name.is_empty() || req.code.trim().is_empty() {
        return fail(
            StatusCode::BAD_REQUEST,
            "give the datacenter a name and its pairing code",
        );
    }
    let Some(address) = clean_address(&req.address) else {
        return fail(
            StatusCode::BAD_REQUEST,
            "the address starts with http:// or https://",
        );
    };
    if st.federation.is_member() {
        return fail(
            StatusCode::CONFLICT,
            "this hub is managed by another one: it cannot control datacenters",
        );
    }
    // checked before the code is used up on a datacenter this hub would then refuse
    if name.eq_ignore_ascii_case(&st.federation.name())
        || st
            .federation
            .datacenters()
            .iter()
            .any(|d| d.name.eq_ignore_ascii_case(&name))
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "another datacenter is already called that",
        );
    }
    let token = match st.links.pair(&address, &req.code, &name).await {
        Ok((token, _)) => token,
        Err(why) => return fail(StatusCode::BAD_GATEWAY, &why),
    };
    let dc = Datacenter {
        id: format!("d{}", rand_hex(4)),
        name,
        address,
        token,
    };
    if let Err(e) = st.federation.add_datacenter(dc.clone()) {
        return fed_error(e);
    }
    st.links.start(&dc.id, &dc.name);
    json_response(StatusCode::CREATED, view(&st))
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct UpdateRequest {
    name: Option<String>,
    address: Option<String>,
}

async fn update_datacenter(
    State(st): State<Shared>,
    _: Admin,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let req: UpdateRequest = match parse(&body) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let address = match req.address.as_deref().map(clean_address) {
        Some(None) => {
            return fail(
                StatusCode::BAD_REQUEST,
                "the address starts with http:// or https://",
            );
        }
        Some(Some(a)) => Some(a),
        None => None,
    };
    match st
        .federation
        .update_datacenter(&id, req.name.as_deref(), address.as_deref())
    {
        Ok(dc) => st.links.renamed(&id, &dc.name),
        Err(e) => return fed_error(e),
    }
    if address.is_some() {
        st.links.reconnect(&id); // dials the new address
    }
    json_response(StatusCode::OK, view(&st))
}

/// Takes a datacenter away: the member goes back to being a hub on its own when it can be told. When it cannot (the link is down) it stays
/// locked, and is released on its own machine (`HUB_FEDERATION=off`).
async fn remove_datacenter(State(st): State<Shared>, _: Admin, Path(id): Path<String>) -> Response {
    let released = st.links.stop(&id, true).await;
    match st.federation.remove_datacenter(&id) {
        Ok(Some(_)) => json_response(StatusCode::OK, json!({"ok": true, "released": released})),
        Ok(None) => fail(StatusCode::NOT_FOUND, "no such datacenter"),
        Err(e) => fed_error(e),
    }
}

/// `/api/dc/{id}/{path}`: the same call, made on the datacenter's own admin API. The datacenter allows only some paths.
async fn tunnel(
    State(st): State<Shared>,
    _: Admin,
    method: Method,
    Path((id, path)): Path<(String, String)>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Response {
    let target = match query {
        Some(q) => format!("/api/{path}?{q}"),
        None => format!("/api/{path}"),
    };
    if !link_allowed(method.as_str(), &target) {
        return fail(StatusCode::FORBIDDEN, "not available on a datacenter");
    }
    match st
        .links
        .request(&id, method.as_str(), &target, body.to_vec())
        .await
    {
        Ok((status, answer)) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            [(header::CONTENT_TYPE, "application/json")],
            answer,
        )
            .into_response(),
        Err(e) => {
            let code = match e {
                LinkError::NoSuchDatacenter => StatusCode::NOT_FOUND,
                LinkError::NotConnected => StatusCode::SERVICE_UNAVAILABLE,
                LinkError::Timeout => StatusCode::GATEWAY_TIMEOUT,
                LinkError::Lost => StatusCode::BAD_GATEWAY,
            };
            fail(code, &e.to_string())
        }
    }
}
