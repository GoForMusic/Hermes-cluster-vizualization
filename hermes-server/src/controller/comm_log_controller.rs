//! The agent ↔ hub communication log, for a logged-in admin only. See `services::comm_log_imp` for what it keeps and why it is off by default.

use std::collections::HashMap;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::json;

use super::{Admin, Shared, fail, json_response, ok, parse};
use crate::model::CommLogSwitch;

pub(super) async fn list_log(
    State(st): State<Shared>,
    _: Admin,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let get = |k: &str| q.get(k).map_or("", String::as_str);
    let since = get("since").parse::<u64>().unwrap_or(0);
    let limit = get("limit").parse::<usize>().unwrap_or(200).clamp(1, 500);
    json_response(
        StatusCode::OK,
        st.comm_log
            .list(since, get("source"), get("kind"), get("q"), limit),
    )
}

pub(super) async fn get_entry(State(st): State<Shared>, _: Admin, Path(id): Path<u64>) -> Response {
    match st.comm_log.get(id) {
        Some(entry) => json_response(StatusCode::OK, entry),
        None => fail(
            StatusCode::NOT_FOUND,
            "that entry is gone (the log keeps the last 500, or fifteen minutes)",
        ),
    }
}

pub(super) async fn switch_log(
    State(st): State<Shared>,
    Admin(user): Admin,
    body: Bytes,
) -> Response {
    let s: CommLogSwitch = match parse(&body) {
        Ok(s) => s,
        Err(r) => return r,
    };
    st.comm_log.set_enabled(s.enabled);
    tracing::info!(
        "{} turned the communication log {}",
        user.username,
        if s.enabled { "on" } else { "off" }
    );
    json_response(StatusCode::OK, json!({"enabled": st.comm_log.enabled()}))
}

pub(super) async fn clear_log(State(st): State<Shared>, _: Admin) -> Response {
    st.comm_log.clear();
    ok()
}
