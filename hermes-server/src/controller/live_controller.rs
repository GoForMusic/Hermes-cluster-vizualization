//! Read-only live data: what version this hub is, the current topology, and the event stream that keeps it live.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
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
pub(super) async fn stream(_: Viewer, State(st): State<Shared>, headers: HeaderMap) -> Response {
    let rx = st.store.subscribe(); // subscribe first so nothing is missed between the snapshot and the stream
    let first = st.store.snapshot_json();
    // A page that keeps the stream open for hours must not outlive its login: once a minute the session is asked about (without counting
    // as the person being active), and a stream whose session ended is closed. The browser then reconnects and is told to log in.
    let token = super::cookie(&headers, super::COOKIE_NAME);
    let auth = st.auth.clone();
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let events = futures_util::stream::unfold(
        (Some(first), rx, tick, token, auth),
        |(first, mut rx, mut tick, token, auth)| async move {
            let data = match first {
                Some(snapshot) => snapshot,
                None => loop {
                    tokio::select! {
                        // too slow (or closed): end the stream, the browser reconnects and gets a fresh snapshot
                        got = rx.recv() => break got.ok()?.to_string(),
                        _ = tick.tick() => {
                            if token.as_deref().is_some_and(|t| !auth.is_alive(t)) {
                                return None;
                            }
                        }
                    }
                },
            };
            Some((
                Ok::<_, Infallible>(SseEvent::default().data(data)),
                (None, rx, tick, token, auth),
            ))
        },
    );
    let sse = Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    );
    ([(HeaderName::from_static("x-accel-buffering"), "no")], sse).into_response()
}
