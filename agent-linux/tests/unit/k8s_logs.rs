//! The log of a pod against a fake API server: it answers the pod and its log the way the real one does, and remembers what was asked.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::*;

#[derive(Default)]
struct Api_ {
    /// the queries the log endpoint got, in order
    asked: Vec<HashMap<String, String>>,
    /// what the log endpoint says instead of lines
    refuse: Option<(u16, String)>,
}
type Shared = Arc<Mutex<Api_>>;

fn pod(name: &str, containers: &[&str]) -> Value {
    json!({
        "apiVersion": "v1", "kind": "Pod", "metadata": {"name": name, "namespace": "shop"},
        "spec": {"containers": containers.iter().map(|c| json!({"name": c, "image": "x"})).collect::<Vec<_>>(),
                 "initContainers": [{"name": "setup", "image": "x"}]},
    })
}

async fn fake(state: Shared) -> SocketAddr {
    async fn get_pod(Path((_ns, name)): Path<(String, String)>) -> Response {
        match name.as_str() {
            "web-1" => Json(pod("web-1", &["app", "sidecar"])).into_response(),
            _ => (
                StatusCode::NOT_FOUND,
                Json(json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": format!("pods \"{name}\" not found"), "reason": "NotFound", "code": 404})),
            )
                .into_response(),
        }
    }
    async fn log(
        State(s): State<Shared>,
        Path((_ns, _name)): Path<(String, String)>,
        Query(q): Query<HashMap<String, String>>,
    ) -> Response {
        let mut s = s.lock().unwrap();
        s.asked.push(q);
        if let Some((code, message)) = s.refuse.clone() {
            return (
                StatusCode::from_u16(code).unwrap(),
                Json(json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": message, "code": code})),
            )
                .into_response();
        }
        "first\nsecond\nthird\n".into_response()
    }
    let app = Router::new()
        .route("/api/v1/namespaces/{ns}/pods/{name}", get(get_pod))
        .route("/api/v1/namespaces/{ns}/pods/{name}/log", get(log))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

async fn rig() -> (K8sLogs, Shared) {
    let state: Shared = Arc::default();
    let addr = fake(state.clone()).await;
    let client =
        Client::try_from(kube::Config::new(format!("http://{addr}").parse().unwrap())).unwrap();
    (K8sLogs::new(client), state)
}

fn query(name: &str) -> LogQuery {
    LogQuery {
        namespace: "shop".into(),
        name: name.into(),
        tail: 100,
        ..LogQuery::default()
    }
}

async fn collect(logs: &K8sLogs, q: LogQuery) -> (Vec<String>, Result<(), LogError>) {
    let (tx, mut rx) = mpsc::channel(16);
    let result = logs.read(q, tx).await;
    let mut lines = Vec::new();
    while let Some(batch) = rx.recv().await {
        lines.extend(batch);
    }
    (lines, result)
}

#[tokio::test]
async fn the_lines_of_the_first_container_are_read_with_what_was_asked_for() {
    let (logs, state) = rig().await;
    let (lines, result) = collect(
        &logs,
        LogQuery {
            previous: true,
            since_seconds: 60,
            timestamps: true,
            ..query("web-1")
        },
    )
    .await;
    result.unwrap();
    assert_eq!(lines, ["first", "second", "third"]);
    let asked = state.lock().unwrap().asked.clone();
    assert_eq!(asked.len(), 1);
    let q = &asked[0];
    assert_eq!(
        (
            q["container"].as_str(),
            q["tailLines"].as_str(),
            q["previous"].as_str(),
            q["sinceSeconds"].as_str(),
            q["timestamps"].as_str()
        ),
        ("app", "100", "true", "60", "true"),
        "the first container is the default"
    );
}

#[tokio::test]
async fn a_container_that_the_pod_has_is_read_and_one_it_does_not_have_says_which_it_has() {
    let (logs, state) = rig().await;
    let (_, result) = collect(
        &logs,
        LogQuery {
            container: "sidecar".into(),
            ..query("web-1")
        },
    )
    .await;
    result.unwrap();
    assert_eq!(state.lock().unwrap().asked[0]["container"], "sidecar");
    let (_, result) = collect(
        &logs,
        LogQuery {
            container: "setup".into(),
            ..query("web-1")
        },
    )
    .await;
    result.unwrap(); // an init container has a log too
    let (lines, result) = collect(
        &logs,
        LogQuery {
            container: "nope".into(),
            ..query("web-1")
        },
    )
    .await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert!(lines.is_empty());
    assert!(
        why.contains("no container \"nope\"") && why.contains("app, sidecar, setup"),
        "{why}"
    );
}

#[tokio::test]
async fn a_pod_that_is_not_there_is_left_for_another_agent_to_find() {
    let (logs, state) = rig().await;
    let (_, result) = collect(&logs, query("ghost")).await;
    let Err(LogError::NotHere(why)) = result else {
        panic!("{result:?}")
    };
    assert_eq!(why, "pod shop/ghost not found");
    assert!(
        state.lock().unwrap().asked.is_empty(),
        "no log was asked for"
    );
}

#[tokio::test]
async fn what_the_api_refuses_is_said_in_words() {
    let (logs, state) = rig().await;
    state.lock().unwrap().refuse = Some((
        403,
        "pods \"web-1\" is forbidden: cannot get resource \"pods/log\"".into(),
    ));
    let (_, result) = collect(&logs, query("web-1")).await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert!(why.contains("Allow reading container logs"), "{why}");

    state.lock().unwrap().refuse = Some((
        400,
        "previous terminated container \"app\" in pod \"web-1\" not found".into(),
    ));
    let (_, result) = collect(
        &logs,
        LogQuery {
            previous: true,
            ..query("web-1")
        },
    )
    .await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        why,
        "this container has not been restarted: there is no earlier log"
    );

    state.lock().unwrap().refuse = Some((
        400,
        "container \"app\" in pod \"web-1\" is waiting to start: ContainerCreating".into(),
    ));
    let (_, result) = collect(&logs, query("web-1")).await;
    let Err(LogError::Failed(why)) = result else {
        panic!("{result:?}")
    };
    assert!(why.contains("waiting to start"), "{why}");
}

#[tokio::test]
async fn a_viewer_that_left_stops_the_read() {
    let (logs, _) = rig().await;
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let (lines, result) = (Vec::<String>::new(), logs.read(query("web-1"), tx).await);
    assert!(
        result.is_ok() && lines.is_empty(),
        "nobody is told anything, and it is not a failure"
    );
}
