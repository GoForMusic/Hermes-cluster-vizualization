//! The hub's own settings blob (alert rules, TV display options, ...).

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{Admin, Shared, Viewer, fail, internal, ok};

pub(super) async fn get_settings(_: Viewer, State(st): State<Shared>) -> Response {
    let raw = st
        .db
        .settings
        .get_setting("settings")
        .unwrap_or_else(|| "{}".into());
    ([(header::CONTENT_TYPE, "application/json")], raw).into_response()
}

pub(super) async fn put_settings(State(st): State<Shared>, _: Admin, body: Bytes) -> Response {
    let Ok(settings) = serde_json::from_slice::<Value>(&body) else {
        return fail(StatusCode::BAD_REQUEST, "settings must be valid JSON");
    };
    if let Err(e) = st
        .db
        .settings
        .set_setting("settings", &String::from_utf8_lossy(&body))
    {
        return internal(e);
    }
    st.rules.set_settings(&body);
    st.store
        .publish(&json!({"type": "settings", "settings": settings}));
    ok()
}
