//! The upgrade against a fake API server that keeps the two objects, applies the strategic-merge patches the way the real one does for
//! this shape, and says whether the rollout became healthy.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::*;

#[derive(Default)]
struct Cluster {
    deployment: Value,
    daemonset: Option<Value>,
    /// the images every patch asked for, in order: `daemonset:..`, `deployment:..`
    patches: Vec<String>,
    /// the rollouts never become healthy
    stuck: bool,
}
type Shared = Arc<Mutex<Cluster>>;

fn workload(kind: &str, container: &str, image: &str) -> Value {
    json!({
        "apiVersion": "apps/v1", "kind": kind, "metadata": {"name": "x", "namespace": "infraviz", "generation": 2},
        "spec": {"replicas": 1, "selector": {"matchLabels": {"a": "b"}}, "template": {"metadata": {}, "spec": {"containers": [{"name": container, "image": image}]}}},
        "status": {}
    })
}

/// Marks the rollout as done (or not) the way the controllers would, from the object's own generation.
fn settle(obj: &mut Value, kind: &str, healthy: bool) {
    let n = i64::from(healthy);
    obj["status"] = if kind == "DaemonSet" {
        json!({"observedGeneration": 2, "desiredNumberScheduled": 3, "currentNumberScheduled": 3, "numberMisscheduled": 0, "numberReady": 3 * n, "updatedNumberScheduled": 3 * n, "numberAvailable": 3 * n})
    } else {
        json!({"observedGeneration": 2, "replicas": 1, "updatedReplicas": n, "availableReplicas": n})
    };
}

async fn fake(cluster: Shared) -> SocketAddr {
    async fn read(
        State(c): State<Shared>,
        Path((kind, _name)): Path<(String, String)>,
    ) -> Result<Json<Value>, (axum::http::StatusCode, Json<Value>)> {
        let mut c = c.lock().unwrap();
        let stuck = c.stuck;
        let obj = if kind == "deployments" {
            Some(&mut c.deployment)
        } else {
            c.daemonset.as_mut()
        };
        let not_found = || {
            (
                axum::http::StatusCode::NOT_FOUND,
                Json(
                    json!({"kind": "Status", "apiVersion": "v1", "status": "Failure", "message": "not found", "reason": "NotFound", "code": 404}),
                ),
            )
        };
        let obj = obj.ok_or_else(not_found)?;
        settle(
            obj,
            if kind == "deployments" {
                "Deployment"
            } else {
                "DaemonSet"
            },
            !stuck,
        );
        Ok(Json(obj.clone()))
    }
    async fn patch(
        State(c): State<Shared>,
        Path((kind, _name)): Path<(String, String)>,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        let mut c = c.lock().unwrap();
        let container = &body["spec"]["template"]["spec"]["containers"][0];
        let image = container["image"].as_str().unwrap().to_string();
        c.patches.push(format!(
            "{}:{image}",
            if kind == "deployments" {
                "deployment"
            } else {
                "daemonset"
            }
        ));
        let obj = if kind == "deployments" {
            &mut c.deployment
        } else {
            c.daemonset.as_mut().unwrap()
        };
        obj["spec"]["template"]["spec"]["containers"][0]["image"] = json!(image);
        Json(obj.clone())
    }
    let app = Router::new()
        .route(
            "/apis/apps/v1/namespaces/infraviz/{kind}/{name}",
            get(read).patch(patch),
        )
        .with_state(cluster);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

async fn rig(cluster: Cluster) -> (K8sUpgrader, Shared) {
    let shared: Shared = Arc::new(Mutex::new(cluster));
    let addr = fake(shared.clone()).await;
    let client =
        Client::try_from(kube::Config::new(format!("http://{addr}").parse().unwrap())).unwrap();
    (K8sUpgrader::new(client, "infraviz").quick(), shared)
}

fn cluster(node: bool) -> Cluster {
    Cluster {
        deployment: workload(
            "Deployment",
            "agent",
            "git.example.com/acm/hermes-agent-linux:1.0.0",
        ),
        daemonset: node.then(|| {
            workload(
                "DaemonSet",
                "node",
                "git.example.com/acm/hermes-agent-linux:1.0.0",
            )
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn the_nodes_are_upgraded_first_and_then_the_agent_itself_keeping_the_repository() {
    let (up, state) = rig(cluster(true)).await;
    assert_eq!(up.upgrade("1.0.2").await.unwrap(), UpgradeOutcome::Started);
    assert_eq!(
        state.lock().unwrap().patches,
        [
            "daemonset:git.example.com/acm/hermes-agent-linux:1.0.2",
            "deployment:git.example.com/acm/hermes-agent-linux:1.0.2",
        ]
    );
}

#[tokio::test]
async fn a_cluster_without_node_agents_is_upgraded_too() {
    let (up, state) = rig(cluster(false)).await;
    assert_eq!(up.upgrade("1.0.2").await.unwrap(), UpgradeOutcome::Started);
    assert_eq!(
        state.lock().unwrap().patches,
        ["deployment:git.example.com/acm/hermes-agent-linux:1.0.2"]
    );
}

#[tokio::test]
async fn the_same_version_again_changes_nothing() {
    let (up, state) = rig(cluster(true)).await;
    up.upgrade("1.0.0")
        .await
        .unwrap_or_else(|e| panic!("{e:#}"));
    assert!(state.lock().unwrap().patches.is_empty());
    assert!(matches!(
        up.upgrade("1.0.0").await.unwrap(),
        UpgradeOutcome::Skipped(_)
    ));
}

#[tokio::test]
async fn nodes_that_do_not_come_up_are_put_back_and_the_agent_is_left_alone() {
    let (up, state) = rig(Cluster {
        stuck: true,
        ..cluster(true)
    })
    .await;
    let e = up.upgrade("1.0.2").await.unwrap_err().to_string();
    assert!(
        e.contains("put back on git.example.com/acm/hermes-agent-linux:1.0.0"),
        "{e}"
    );
    assert_eq!(
        state.lock().unwrap().patches,
        [
            "daemonset:git.example.com/acm/hermes-agent-linux:1.0.2",
            "daemonset:git.example.com/acm/hermes-agent-linux:1.0.0",
        ]
    );
}

#[tokio::test]
async fn an_agent_that_never_gets_ready_puts_the_old_image_back() {
    let (up, state) = rig(Cluster {
        stuck: true,
        ..cluster(false)
    })
    .await;
    assert_eq!(up.upgrade("1.0.2").await.unwrap(), UpgradeOutcome::Started);
    for _ in 0..100 {
        if state.lock().unwrap().patches.len() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        state.lock().unwrap().patches,
        [
            "deployment:git.example.com/acm/hermes-agent-linux:1.0.2",
            "deployment:git.example.com/acm/hermes-agent-linux:1.0.0",
        ]
    );
}

#[tokio::test]
async fn a_version_that_is_not_one_is_refused_before_anything_is_touched() {
    let (up, state) = rig(cluster(true)).await;
    assert!(up.upgrade("../../evil").await.is_err());
    assert!(state.lock().unwrap().patches.is_empty());
}
