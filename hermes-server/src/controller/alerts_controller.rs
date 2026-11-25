//! Alerts and the uptime bars they are recorded into.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;

use super::{Admin, Shared, Viewer, fail, internal, json_response, ok};
use crate::database::now_ms;

fn number(query: &HashMap<String, String>, key: &str) -> i64 {
    query.get(key).and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub(super) async fn list_alerts(
    Viewer(sight): Viewer,
    State(st): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let limit = number(&query, "limit");
    let limit = if (1..=1000).contains(&limit) {
        limit
    } else {
        300
    };
    match st.db.alerts.list_alerts(limit) {
        // the alerts of the clusters this person may see, and none of the others
        Ok(list) => json_response(
            StatusCode::OK,
            list.into_iter()
                .filter(|a| sight.sees_node(&a.node_id))
                .collect::<Vec<_>>(),
        ),
        Err(e) => internal(e),
    }
}

pub(super) async fn ack_alert(
    State(st): State<Shared>,
    Admin(user): Admin,
    Path(id): Path<String>,
) -> Response {
    match id.parse::<i64>() {
        Ok(id) if st.rules.ack(id, &user.username) => ok(),
        _ => fail(StatusCode::NOT_FOUND, "no such alert"),
    }
}

pub(super) async fn uptime(
    Viewer(sight): Viewer,
    State(st): State<Shared>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let span = Some(number(&query, "span"))
        .filter(|s| *s > 0)
        .unwrap_or(3600);
    let buckets = Some(number(&query, "buckets"))
        .filter(|b| (1..=200).contains(b))
        .unwrap_or(48);
    match st.db.beats.uptime(
        now_ms(),
        span * 1000,
        usize::try_from(buckets).unwrap_or(48),
    ) {
        Ok(res) => json_response(
            StatusCode::OK,
            res.into_iter()
                .filter(|(id, _)| sight.sees_node(id))
                .collect::<std::collections::HashMap<_, _>>(),
        ),
        Err(e) => internal(e),
    }
}
