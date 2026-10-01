//! The upgrade against a fake Docker engine on a unix socket: it keeps the services, and records what the agent POSTs to update them.

use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use super::*;
use crate::engine::connector_for;

#[derive(Default)]
struct Engine_ {
    manager: bool,
    stack_label: bool,
    with_windows: bool,
    /// (service, version index asked for, the whole spec sent), in order
    updates: Vec<(String, u64, Value)>,
}
type Shared = Arc<Mutex<Engine_>>;

fn service(name: &str, image: &str) -> Value {
    json!({"ID": format!("id-{name}"), "Version": {"Index": 7}, "Spec": {"Name": name, "TaskTemplate": {"ContainerSpec": {"Image": image}}, "Mode": {"Global": {}}}})
}

async fn rig(state: Engine_) -> (SwarmUpgrader, Shared, tempfile::TempDir) {
    let shared: Shared = Arc::new(Mutex::new(state));
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("docker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let app = Router::new()
        .route(
            "/info",
            get(|State(s): State<Shared>| async move {
                Json(json!({"Swarm": {"ControlAvailable": s.lock().unwrap().manager}}))
            }),
        )
        .route(
            "/containers/{id}/json",
            get(
                |State(s): State<Shared>, Path(_id): Path<String>| async move {
                    let labels = if s.lock().unwrap().stack_label {
                        json!({NAMESPACE_LABEL: "infraviz"})
                    } else {
                        json!({})
                    };
                    Json(json!({"Config": {"Labels": labels}}))
                },
            ),
        )
        .route(
            "/services/{name}",
            get(
                |State(s): State<Shared>, Path(name): Path<String>| async move {
                    let with_windows = s.lock().unwrap().with_windows;
                    match name.as_str() {
                        // Swarm keeps the digest it resolved next to the tag
                        "infraviz_agent" => Ok(Json(service(
                            &name,
                            "git.example.com/acm/hermes-agent-linux:1.0.0@sha256:abcd",
                        ))),
                        "infraviz_agent-windows" if with_windows => Ok(Json(service(
                            &name,
                            "git.example.com/acm/hermes-agent-windows:1.0.0-ltsc2022",
                        ))),
                        _ => Err((
                            axum::http::StatusCode::NOT_FOUND,
                            Json(json!({"message": "service not found"})),
                        )),
                    }
                },
            ),
        )
        .route(
            "/services/{id}/update",
            post(
                |State(s): State<Shared>,
                 Path(id): Path<String>,
                 Query(q): Query<std::collections::HashMap<String, String>>,
                 Json(spec): Json<Value>| async move {
                    s.lock()
                        .unwrap()
                        .updates
                        .push((id, q["version"].parse().unwrap(), spec));
                    Json(json!({"Warnings": null}))
                },
            ),
        )
        .with_state(shared.clone());
    tokio::spawn(axum::serve(listener, app).into_future());
    let engine = Arc::new(Engine::new(connector_for(&format!(
        "unix://{}",
        socket.display()
    ))));
    (SwarmUpgrader::new(engine), shared, dir)
}

#[tokio::test]
async fn the_services_get_the_new_tag_with_a_rolling_update_that_rolls_back() {
    let (up, state, _dir) = rig(Engine_ {
        manager: true,
        stack_label: true,
        with_windows: true,
        ..Default::default()
    })
    .await;
    assert_eq!(up.upgrade("1.0.2").await.unwrap(), UpgradeOutcome::Started);
    let s = state.lock().unwrap();
    // Windows first, the service this agent runs in last
    let names: Vec<&str> = s.updates.iter().map(|u| u.0.as_str()).collect();
    assert_eq!(names, ["id-infraviz_agent-windows", "id-infraviz_agent"]);
    assert_eq!(
        s.updates[0].2["TaskTemplate"]["ContainerSpec"]["Image"],
        "git.example.com/acm/hermes-agent-windows:1.0.2-ltsc2022"
    );
    let linux = &s.updates[1];
    assert_eq!(
        linux.1, 7,
        "the version index of the service as it was read"
    );
    assert_eq!(
        linux.2["TaskTemplate"]["ContainerSpec"]["Image"],
        "git.example.com/acm/hermes-agent-linux:1.0.2",
        "the digest of the old tag is dropped"
    );
    assert_eq!(linux.2["UpdateConfig"]["FailureAction"], "rollback");
    assert_eq!(linux.2["UpdateConfig"]["Order"], "start-first");
    assert_eq!(
        linux.2["Mode"],
        json!({"Global": {}}),
        "the rest of the spec is kept"
    );
}

#[tokio::test]
async fn a_stack_without_windows_nodes_only_has_the_linux_service() {
    let (up, state, _dir) = rig(Engine_ {
        manager: true,
        stack_label: true,
        ..Default::default()
    })
    .await;
    assert_eq!(up.upgrade("1.0.2").await.unwrap(), UpgradeOutcome::Started);
    assert_eq!(state.lock().unwrap().updates.len(), 1);
}

#[tokio::test]
async fn a_worker_leaves_it_to_a_manager() {
    let (up, state, _dir) = rig(Engine_ {
        manager: false,
        stack_label: true,
        ..Default::default()
    })
    .await;
    assert!(matches!(
        up.upgrade("1.0.2").await.unwrap(),
        UpgradeOutcome::Skipped(_)
    ));
    assert!(state.lock().unwrap().updates.is_empty());
}

#[tokio::test]
async fn the_same_tag_changes_nothing_and_an_agent_that_is_not_a_stack_is_refused() {
    let (up, state, _dir) = rig(Engine_ {
        manager: true,
        stack_label: true,
        ..Default::default()
    })
    .await;
    assert!(matches!(
        up.upgrade("1.0.0").await.unwrap(),
        UpgradeOutcome::Skipped(_)
    ));
    assert!(state.lock().unwrap().updates.is_empty());
    let (up, _state, _dir) = rig(Engine_ {
        manager: true,
        stack_label: false,
        ..Default::default()
    })
    .await;
    let e = format!("{:#}", up.upgrade("1.0.2").await.unwrap_err());
    assert!(e.contains("not deployed as a stack"), "{e}");
}

#[tokio::test]
async fn a_version_that_is_not_one_is_refused() {
    let (up, state, _dir) = rig(Engine_ {
        manager: true,
        stack_label: true,
        ..Default::default()
    })
    .await;
    assert!(up.upgrade("latest").await.is_err());
    assert!(state.lock().unwrap().updates.is_empty());
}
