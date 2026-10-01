//! Read-only live data: what version this hub is, the current topology, and the event stream that keeps it live.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::http::{HeaderName, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};

use super::{Shared, Viewer, json_response};
use crate::manifest;
use crate::model::HubInfo;
use crate::version::VERSION;

pub(super) fn source_types() -> Vec<&'static str> {
    manifest::agent_types() // the hub has no collector of its own yet: every source is an agent
}

pub(super) async fn info(_: Viewer) -> Response {
    let info = HubInfo {
        demo: false,
        source_types: source_types().into_iter().map(String::from).collect(),
        version: VERSION.to_string(),
    };
    json_response(StatusCode::OK, info)
}

pub(super) async fn snapshot(_: Viewer, State(st): State<Shared>) -> Response {
    (
        [(header::CONTENT_TYPE, "application/json")],
        st.store.snapshot_json(),
    )
        .into_response()
}

/// Server-Sent Events: one JSON event per `data:` line. The browser's `EventSource` reconnects by itself, and every new connection
/// starts with a full snapshot.
pub(super) async fn stream(_: Viewer, State(st): State<Shared>) -> Response {
    let rx = st.store.subscribe(); // subscribe first so nothing is missed between the snapshot and the stream
    let first = st.store.snapshot_json();
    let events = futures_util::stream::unfold((Some(first), rx), |(first, mut rx)| async move {
        let data = match first {
            Some(snapshot) => snapshot,
            // too slow (or closed): end the stream, the browser reconnects and gets a fresh snapshot
            None => rx.recv().await.ok()?.to_string(),
        };
        Some((
            Ok::<_, Infallible>(SseEvent::default().data(data)),
            (None, rx),
        ))
    });
    let sse = Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    );
    ([(HeaderName::from_static("x-accel-buffering"), "no")], sse).into_response()
}
