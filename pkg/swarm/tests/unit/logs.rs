//! The log of a task or a container against a fake engine on a unix socket: frames, a terminal's plain text, a manager and a worker.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::*;
use crate::engine::connector_for;

/// One frame as the engine writes it: the stream (1 stdout, 2 stderr), three zeros, the length, the text.
fn frame(stream: u8, text: &str) -> Vec<u8> {
    let mut f = vec![stream, 0, 0, 0];
    f.extend_from_slice(&(text.len() as u32).to_be_bytes());
    f.extend_from_slice(text.as_bytes());
    f
}

#[test]
fn frames_become_lines_whatever_way_they_are_cut() {
    let mut bytes = frame(1, "one\ntwo\n");
    bytes.extend(frame(2, "err\n"));
    bytes.extend(frame(1, "no line break at the end"));
    let all: Vec<String> = {
        let mut d = Decoder::default();
        let mut lines = d.push(&bytes);
        lines.extend(d.finish());
        lines
    };
    assert_eq!(all, ["one", "two", "err", "no line break at the end"]);
    // the same bytes, a few at a time: the answer does not depend on where the network cut them
    for size in [1, 3, 7, 8, 9, 20] {
        let mut d = Decoder::default();
        let mut lines = Vec::new();
        for chunk in bytes.chunks(size) {
            lines.extend(d.push(chunk));
        }
        lines.extend(d.finish());
        assert_eq!(lines, all, "chunks of {size}");
    }
}

#[test]
fn a_container_with_a_terminal_sends_plain_text_and_a_line_may_end_in_a_carriage_return() {
    let mut d = Decoder::default();
    let mut lines = d.push(b"hello\r\nwor");
    lines.extend(d.push(b"ld\nlast"));
    lines.extend(d.finish());
    assert_eq!(lines, ["hello", "world", "last"]);
    // text that starts with a byte that could be a frame header but is not one
    let mut d = Decoder::default();
    assert_eq!(d.push(b"\x01abc\n"), ["\u{1}abc"]);
}

#[test]
fn a_line_is_text_even_when_the_bytes_are_not() {
    let mut d = Decoder::default();
    let lines = d.push(b"caf\xc3\xa9 \xff\xfe\n");
    assert_eq!(lines, ["café \u{fffd}\u{fffd}"]);
}

#[derive(Default)]
struct Engine_ {
    /// what the log endpoints were asked: (path, query)
    asked: Vec<(String, HashMap<String, String>)>,
}
type Shared = Arc<Mutex<Engine_>>;

fn task(id: &str, service: &str, slot: i64, node: &str, created: &str) -> Value {
    json!({"ID": id, "ServiceID": service, "Slot": slot, "NodeID": node, "CreatedAt": created})
}

async fn fake(manager: bool, state: Shared) -> (tempfile::TempDir, Arc<crate::engine::Engine>) {
    async fn one(State((manager, _)): State<(bool, Shared)>, Path(id): Path<String>) -> Response {
        if !manager {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "This node is not a swarm manager. Use \"docker swarm init\".",
            )
                .into_response();
        }
        match id.as_str() {
            "t3" => axum::Json(task("t3", "svc", 1, "n1", "2026-05-03T10:00:00Z")).into_response(),
            "t1" | "t2" => axum::Json(task(
                &id,
                "svc",
                1,
                "n1",
                if id == "t1" {
                    "2026-05-01T10:00:00Z"
                } else {
                    "2026-05-02T10:00:00Z"
                },
            ))
            .into_response(),
            "lonely" => {
                axum::Json(task("lonely", "alone", 1, "n1", "2026-05-01T10:00:00Z")).into_response()
            }
            _ => (StatusCode::NOT_FOUND, "no such task").into_response(),
        }
    }
    async fn list(
        State((_, _)): State<(bool, Shared)>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        let filters: Value = serde_json::from_str(&q["filters"]).unwrap();
        let service = filters["service"][0].as_str().unwrap();
        let all = vec![
            task("t1", "svc", 1, "n1", "2026-05-01T10:00:00Z"),
            task("t2", "svc", 1, "n1", "2026-05-02T10:00:00Z"),
            task("t3", "svc", 1, "n1", "2026-05-03T10:00:00Z"),
            task("other-slot", "svc", 2, "n2", "2026-05-02T12:00:00Z"),
            task("lonely", "alone", 1, "n1", "2026-05-01T10:00:00Z"),
        ];
        axum::Json(
            all.into_iter()
                .filter(|t| t["ServiceID"] == service)
                .collect::<Vec<_>>(),
        )
        .into_response()
    }
    async fn task_log(
        State((_, s)): State<(bool, Shared)>,
        Path(id): Path<String>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        s.lock()
            .unwrap()
            .asked
            .push((format!("/tasks/{id}/logs"), q));
        let mut body = frame(1, &format!("log of {id}\n"));
        body.extend(frame(2, "an error line\n"));
        body.into_response()
    }
    async fn container_log(
        State((_, s)): State<(bool, Shared)>,
        Path(name): Path<String>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        s.lock()
            .unwrap()
            .asked
            .push((format!("/containers/{name}/logs"), q));
        format!("plain log of {name}\n").into_response()
    }
    let app = Router::new()
        .route("/tasks/{id}", get(one))
        .route("/tasks", get(list))
        .route("/tasks/{id}/logs", get(task_log))
        .route("/containers/{name}/logs", get(container_log))
        .with_state((manager, state));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("docker.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    (
        dir,
        Arc::new(Engine::new(connector_for(&format!(
            "unix://{}",
            path.display()
        )))),
    )
}

async fn read(logs: &DockerLogs, q: LogQuery) -> (Vec<String>, Result<(), LogError>) {
    let (tx, mut rx) = mpsc::channel(16);
    let result = logs.read(q, tx).await;
    let mut lines = Vec::new();
    while let Some(batch) = rx.recv().await {
        lines.extend(batch);
    }
    (lines, result)
}

fn query(task: &str) -> LogQuery {
    LogQuery {
        namespace: "stack".into(),
        name: "web.1".into(),
        tail: 50,
        task: task.into(),
        ..LogQuery::default()
    }
}

#[tokio::test]
async fn a_task_is_read_through_the_manager_with_what_was_asked_for() {
    let state: Shared = Arc::default();
    let (_dir, engine) = fake(true, state.clone()).await;
    let logs = DockerLogs::new(engine, Mode::Swarm);
    let (lines, result) = read(
        &logs,
        LogQuery {
            follow: true,
            timestamps: true,
            since_seconds: 30,
            ..query("t3")
        },
    )
    .await;
    result.unwrap();
    assert_eq!(lines, ["log of t3", "an error line"]);
    let (path, q) = state.lock().unwrap().asked[0].clone();
    assert_eq!(path, "/tasks/t3/logs");
    assert_eq!(
        (
            q["tail"].as_str(),
            q["follow"].as_str(),
            q["timestamps"].as_str(),
            q["stdout"].as_str(),
            q["stderr"].as_str()
        ),
        ("50", "1", "1", "1", "1")
    );
    assert!(q["since"].parse::<u64>().unwrap() > 1_700_000_000);
}

#[tokio::test]
async fn the_previous_log_is_the_one_of_the_task_that_ran_this_slot_before() {
    let state: Shared = Arc::default();
    let (_dir, engine) = fake(true, state.clone()).await;
    let logs = DockerLogs::new(engine, Mode::Swarm);
    let (lines, result) = read(
        &logs,
        LogQuery {
            previous: true,
            ..query("t3")
        },
    )
    .await;
    result.unwrap();
    assert_eq!(
        lines[0], "log of t2",
        "not t1 (older), not the task of another slot, and not itself"
    );
    let (_, result) = read(
        &logs,
        LogQuery {
            previous: true,
            ..query("lonely")
        },
    )
    .await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert!(why.contains("no earlier one"), "{why}");
}

#[tokio::test]
async fn a_worker_is_not_the_one_to_ask_and_a_task_that_is_gone_is_for_another_to_find() {
    let state: Shared = Arc::default();
    let (_dir, engine) = fake(false, state.clone()).await;
    let (_, result) = read(&DockerLogs::new(engine, Mode::Swarm), query("t3")).await;
    let Err(LogError::NotHere(why)) = result else {
        panic!("{result:?}")
    };
    assert_eq!(why, "this node is not a swarm manager");

    let (_dir, engine) = fake(true, state.clone()).await;
    let logs = DockerLogs::new(engine, Mode::Swarm);
    let (_, result) = read(&logs, query("gone")).await;
    assert!(matches!(result, Err(LogError::NotHere(_))), "{result:?}");
    let (_, result) = read(&logs, query("")).await;
    assert!(
        matches!(result, Err(LogError::Failed(_))),
        "a task id is needed"
    );
    assert!(
        state.lock().unwrap().asked.is_empty(),
        "no log was asked for"
    );
}

#[tokio::test]
async fn a_docker_machine_serves_its_containers_by_name_and_has_no_earlier_log() {
    let state: Shared = Arc::default();
    let (_dir, engine) = fake(true, state.clone()).await;
    let logs = DockerLogs::new(engine, Mode::Docker);
    let (lines, result) = read(
        &logs,
        LogQuery {
            name: "my app".into(),
            ..query("")
        },
    )
    .await;
    result.unwrap();
    assert_eq!(lines, ["plain log of my app"]);
    assert_eq!(state.lock().unwrap().asked[0].0, "/containers/my app/logs");
    let (_, result) = read(
        &logs,
        LogQuery {
            previous: true,
            ..query("")
        },
    )
    .await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert!(why.contains("one log for a container"), "{why}");
}
