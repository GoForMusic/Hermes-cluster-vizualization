//! The log of a container, streamed to the browser as it is read: `GET /api/logs/{source}/{namespace}/{name}`. The policy path is the URL, so a
//! rule like `path "logs/prod/payments/*" { capabilities = ["read"] }` says who may read what.
//!
//! Nothing is stored here. The request goes to an agent (see `LogServiceImp`), the lines come back and go to the browser, and a browser that
//! leaves (or a limit that is reached) ends it. The limits keep one viewer from costing the hub or the agent more than it can spare.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use super::{Admin, Shared, fail};
use crate::services::{LogEvent, LogOpenError, LogOptions};

/// The logs read at once over the whole hub.
pub const MAX_LOG_STREAMS: usize = 16;
/// A followed log ends after this long: whoever still wants it opens it again.
const MAX_FOLLOW: Duration = Duration::from_secs(30 * 60);
/// A log ends after this much text.
const MAX_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_TAIL: u32 = 200;

#[derive(Deserialize, Default)]
#[serde(default)]
pub(super) struct Params {
    container: String,
    tail: Option<u32>,
    follow: bool,
    previous: bool,
    since: u32,
    timestamps: bool,
    /// Swarm: the exact task (a name is not unique over time).
    task: String,
}

fn plain(s: &str) -> bool {
    !s.is_empty() && s.len() <= 253 && !s.chars().any(char::is_control)
}

pub(super) async fn read_log(
    State(st): State<Shared>,
    Admin(user): Admin,
    Path((source, target)): Path<(String, String)>,
    Query(q): Query<Params>,
) -> Response {
    let mut parts = target.split('/');
    let (Some(namespace), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return fail(
            StatusCode::BAD_REQUEST,
            "the log of a workload is /api/logs/<source>/<namespace>/<name>",
        );
    };
    if ![namespace, name, q.container.as_str(), q.task.as_str()]
        .iter()
        .all(|s| s.is_empty() || plain(s))
        || !plain(namespace)
        || !plain(name)
    {
        return fail(
            StatusCode::BAD_REQUEST,
            "that is not the name of a workload",
        );
    }
    if !st
        .db
        .sources
        .list_sources()
        .unwrap_or_default()
        .iter()
        .any(|s| s.id == source)
    {
        return fail(StatusCode::NOT_FOUND, "no such source");
    }
    let Ok(slot) = st.log_slots.clone().try_acquire_owned() else {
        return fail(
            StatusCode::TOO_MANY_REQUESTS,
            "too many logs are being read at once: close one",
        );
    };
    let options = LogOptions {
        namespace: namespace.into(),
        name: name.into(),
        container: q.container.clone(),
        tail: q.tail.unwrap_or(DEFAULT_TAIL).clamp(1, 5000),
        follow: q.follow,
        previous: q.previous,
        since_seconds: q.since,
        timestamps: q.timestamps,
        task: q.task.clone(),
    };
    // who read what: the log can hold what the policies would have kept from them
    info!(
        "audit: {} reads the log of {source}/{namespace}/{name} (container {:?}, tail {}, follow {}, previous {})",
        user.username, options.container, options.tail, options.follow, options.previous
    );
    let stream = match st.logs.open(&source, &options).await {
        Ok(s) => s,
        Err(LogOpenError::NoCapableAgent) => {
            return fail(
                StatusCode::CONFLICT,
                "no agent of this source was installed to read logs: add the source again with \"Allow reading container logs\" ticked and apply that manifest once",
            );
        }
        Err(LogOpenError::NotFound(why)) => {
            let why = if why.is_empty() {
                "no agent of this source has it".to_string()
            } else {
                why
            };
            return fail(StatusCode::NOT_FOUND, &why);
        }
        Err(LogOpenError::Failed(why)) => return fail(StatusCode::BAD_GATEWAY, &why),
    };
    let deadline = q.follow.then(|| tokio::time::Instant::now() + MAX_FOLLOW);
    // the state: the log, how much was sent, when a followed log ends, the slot (held until the stream is dropped), and where it is up to
    let events = futures_util::stream::unfold(
        (stream, 0usize, deadline, slot, Phase::Reading),
        |(mut stream, sent, deadline, slot, phase)| async move {
            let failed = |why: &str| {
                SseEvent::default()
                    .event("error")
                    .data(json!({"error": why}).to_string())
            };
            match phase {
                Phase::Done => None,
                Phase::Cut => Some((
                    Ok::<_, Infallible>(failed(
                        "this log was cut at 8 MiB: narrow it with a smaller tail, or the search",
                    )),
                    (stream, sent, deadline, slot, Phase::Done),
                )),
                Phase::Reading => {
                    let next = match deadline {
                        Some(at) => tokio::time::timeout_at(at, stream.next())
                            .await
                            .unwrap_or_else(|_| {
                                Some(LogEvent::Failed(
                                    "this log was followed for 30 minutes: open it again to go on"
                                        .into(),
                                ))
                            }),
                        None => stream.next().await,
                    }?;
                    Some(match next {
                        LogEvent::Lines(lines) => {
                            let sent = sent + lines.iter().map(String::len).sum::<usize>();
                            let phase = if sent > MAX_BYTES {
                                Phase::Cut
                            } else {
                                Phase::Reading
                            };
                            (
                                Ok(SseEvent::default().data(json!({"lines": lines}).to_string())),
                                (stream, sent, deadline, slot, phase),
                            )
                        }
                        LogEvent::End => (
                            Ok(SseEvent::default().event("end").data("{}")),
                            (stream, sent, deadline, slot, Phase::Done),
                        ),
                        LogEvent::Failed(why) => (
                            Ok(failed(&why)),
                            (stream, sent, deadline, slot, Phase::Done),
                        ),
                    })
                }
            }
        },
    );
    let sse = Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    );
    ([(HeaderName::from_static("x-accel-buffering"), "no")], sse).into_response()
}

#[derive(Clone, Copy)]
enum Phase {
    Reading,
    /// Too much was sent: the notice goes next, and then it is over.
    Cut,
    Done,
}
