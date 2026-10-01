//! The collector as a whole, against a fake Docker engine on a unix socket: the real HTTP client, the real JSON, both kinds of node.
//! Unix only: the fake engine listens on a real `UnixListener`, which doesn't exist on Windows (the production code's
//! Windows named-pipe path is covered instead by `engine.rs`'s own `an_endpoint_the_platform_cannot_open_says_so_when_used`).
#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use hermes_agentkit::SharedSink;
use hermes_agentkit::host::{HostSample, ISampler};
use hermes_agentkit::testing::{Call, RecordingSink};
use hermes_proto::v1::{CollectorState, NodeKind};
use hermes_swarm::engine::{Engine, connector_for};
use serde_json::{Value, json};

struct FixedHost;

impl ISampler for FixedHost {
    fn sample(&mut self) -> anyhow::Result<HostSample> {
        Ok(HostSample {
            cpu: Some(25.0),
            mem_used_mib: 1024.0,
            mem_total_mib: 4096.0,
        })
    }
}

fn info(control: bool, node_id: &str) -> Value {
    json!({"ServerVersion": "27.3.1", "NCPU": 2, "OSType": "linux", "MemTotal": 4294967296u64,
           "Swarm": {"NodeID": node_id, "ControlAvailable": control, "Cluster": if control { json!({"ID": "swarm-uid"}) } else { Value::Null }}})
}

fn serve(dir: &Path, info: Value) -> String {
    let socket = dir.join("docker.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let node = |id: &str, role: &str, host: &str| json!({"ID": id, "Spec": {"Role": role}, "Status": {"State": "ready", "Addr": "10.0.0.1"}, "Description": {"Hostname": host, "Platform": {"OS": "linux", "Architecture": "x86_64"}}});
    let task = json!({"ID": "task0000000000000000", "ServiceID": "svc", "NodeID": "m1", "Slot": 1, "DesiredState": "running", "Status": {"State": "running"}, "Spec": {"ContainerSpec": {"Image": "nginx:alpine@sha256:abc"}}, "NetworksAttachments": [{"Network": {"ID": "net-app"}, "Addresses": ["10.0.2.4/24"]}, {"Network": {"ID": "net-ingress"}}]});
    let stats = json!({
        "read": "2026-01-01T12:00:01Z",
        "cpu_stats": {"cpu_usage": {"total_usage": 2_000_000}, "system_cpu_usage": 20_000_000, "online_cpus": 2},
        "precpu_stats": {"cpu_usage": {"total_usage": 1_000_000}, "system_cpu_usage": 10_000_000},
        "memory_stats": {"usage": 100 << 20, "stats": {"inactive_file": 40 << 20}},
        "networks": {"eth0": {"rx_bytes": 10, "tx_bytes": 20}},
    });
    let app = Router::new()
        .route("/info", get(move || { let i = info.clone(); async move { axum::Json(i) } }))
        .route("/nodes", get({ let n = json!([node("m1", "manager", "mgr"), node("w1", "worker", "wrk")]); move || { let n = n.clone(); async move { axum::Json(n) } } }))
        .route("/services", get(|| async { axum::Json(json!([{"ID": "svc", "Spec": {"Name": "web", "Mode": {"Replicated": {}}}}])) }))
        .route("/tasks", get({ let t = json!([task]); move || { let t = t.clone(); async move { axum::Json(t) } } }))
        .route("/networks", get(|| async { axum::Json(json!([
            {"Id": "net-app", "Name": "demo_net", "Driver": "overlay", "Scope": "swarm", "IPAM": {"Config": [{"Subnet": "10.0.2.0/24"}]}, "Options": {"com.docker.network.driver.overlay.vxlanid_list": "4097"}},
            {"Id": "net-ingress", "Name": "ingress", "Driver": "overlay", "Scope": "swarm", "Ingress": true},
            {"Id": "net-bridge", "Name": "bridge", "Driver": "bridge", "Scope": "local"}])) }))
        .route("/containers/json", get(|| async { axum::Json(json!([{"Id": "c1", "Labels": {"com.docker.swarm.task.id": "task0000000000000000", "com.docker.swarm.service.name": "web"}, "Mounts": [{"Type": "volume", "Name": "demo_data"}, {"Type": "bind", "Name": ""}]}, {"Id": "c2", "Labels": {}}])) }))
        .route("/system/df", get(|| async { axum::Json(json!({"Volumes": [
            {"Name": "demo_data", "Driver": "local", "Labels": {"com.docker.stack.namespace": "demo"}, "UsageData": {"Size": 1073741824u64, "RefCount": 1}},
            {"Name": "a".repeat(64), "Driver": "local", "UsageData": {"Size": 5, "RefCount": 1}}
        ]})) }))
        .route("/containers/{id}/stats", get(move || { let s = stats.clone(); async move { axum::Json(s) } }));
    tokio::spawn(axum::serve(listener, app).into_future());
    format!("unix://{}", socket.display())
}

/// Runs the collector until `done` says the sink has what a test wants, or a few seconds pass; returns the collector's result if it ended.
async fn collect(
    endpoint: &str,
    done: impl Fn(&[Call]) -> bool,
) -> (Arc<RecordingSink>, Option<anyhow::Result<()>>) {
    let sink = Arc::new(RecordingSink::default());
    let engine = Arc::new(Engine::new(connector_for(endpoint)));
    let shared: SharedSink = sink.clone();
    let mut task = tokio::spawn(async move {
        hermes_swarm::run("s1", "lab", engine, Box::new(FixedHost), shared).await
    });
    let mut ended = None;
    for _ in 0..150 {
        if task.is_finished() {
            ended = Some((&mut task).await.unwrap());
            break;
        }
        if done(&sink.calls()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    (sink, ended)
}

#[tokio::test]
async fn a_manager_reports_the_topology_and_what_it_measures_about_itself_and_its_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, _) = collect(&serve(dir.path(), info(true, "m1")), |calls| {
        calls.iter().any(|c| matches!(c, Call::Topology(..)))
            && calls.iter().any(|c| matches!(c, Call::Metrics(..)))
            && calls.iter().any(|c| matches!(c, Call::Alive(_)))
            && calls.iter().any(|c| matches!(c, Call::Contribution(_)))
    })
    .await;
    let calls = sink.calls();

    let Some(Call::Topology(nodes, edges)) = calls
        .iter()
        .find(|c| matches!(c, Call::Topology(..)))
        .cloned()
    else {
        panic!("no topology")
    };
    assert_eq!(
        nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        [
            "s1",
            "s1:n:m1",
            "s1:n:w1",
            "s1:t:task0000000000000000",
            "s1:net:net-app"
        ],
        "the ingress mesh and the per-node bridge are not drawn"
    );
    assert_eq!(nodes[3].kind(), NodeKind::Workload);
    assert_eq!(nodes[4].kind(), NodeKind::Network);
    assert_eq!(edges.len(), 2, "the control link and the network route");
    let report = calls
        .iter()
        .find_map(|c| {
            if let Call::Report(s, i) = c {
                Some((*s, i.clone()))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        report,
        (
            CollectorState::Connected,
            "2 nodes · 1 services · 1 tasks".to_string()
        )
    );

    let Some(Call::Metrics(m, _)) = calls
        .iter()
        .find(|c| matches!(c, Call::Metrics(..)))
        .cloned()
    else {
        panic!("no metrics")
    };
    let host = &m["s1:n:m1"];
    assert_eq!(
        (host["cpu"], host["cpuMilli"], host["mem"], host["memMiB"]),
        (25.0, 500.0, 25.0, 1024.0),
        "this node's own CPU and memory, from the sampler"
    );
    let task = &m["s1:t:task0000000000000000"];
    assert_eq!(
        (task["cpuMilli"], task["memMiB"], task["mem"]),
        (200.0, 60.0, 60.0 / 4096.0 * 100.0),
        "the task's container, measured by the engine's stats"
    );
    assert!(!task.contains_key("rxMbps"), "a rate needs two samples");

    // the volumes of this node: a contribution to the topology, and their usage with the numbers
    let Some(Call::Contribution(volumes)) = calls
        .iter()
        .find(|c| matches!(c, Call::Contribution(_)))
        .cloned()
    else {
        panic!("no volumes contributed")
    };
    assert_eq!(
        volumes.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
        ["s1:v:m1:demo_data"],
        "the anonymous volume is left out"
    );
    assert_eq!(volumes[0].parent.as_deref(), Some("s1:n:m1"));
    assert_eq!(m["s1:v:m1:demo_data"]["used"], 1.0, "1 GiB");

    let Some(Call::Alive(ids)) = calls.iter().find(|c| matches!(c, Call::Alive(_))).cloned() else {
        panic!("no alive")
    };
    assert_eq!(
        ids,
        ["s1:t:task0000000000000000"],
        "a container that is not a swarm task is not counted"
    );
}

#[tokio::test]
async fn a_worker_reports_only_what_it_measures() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, _) = collect(&serve(dir.path(), info(false, "w1")), |calls| {
        calls.iter().any(|c| matches!(c, Call::Metrics(..)))
    })
    .await;
    let calls = sink.calls();
    assert!(
        !calls.iter().any(|c| matches!(c, Call::Topology(..))),
        "only a manager sees services and tasks"
    );
    assert!(calls.contains(&Call::Report(
        CollectorState::Connected,
        "worker node: reporting CPU and memory only".into()
    )));
    let Some(Call::Metrics(m, _)) = calls
        .iter()
        .find(|c| matches!(c, Call::Metrics(..)))
        .cloned()
    else {
        panic!("no metrics")
    };
    assert!(m.contains_key("s1:n:w1"));
}

#[tokio::test]
async fn an_engine_that_is_not_in_a_swarm_is_an_error_that_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (_, ended) = collect(&serve(dir.path(), info(false, "")), |_| false).await;
    assert!(
        format!("{:#}", ended.expect("it must stop").unwrap_err()).contains("not part of a swarm")
    );
}

#[tokio::test]
async fn an_engine_that_is_not_there_is_an_error_that_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (_, ended) = collect(
        &format!("unix://{}/none.sock", dir.path().display()),
        |_| false,
    )
    .await;
    assert!(
        format!("{:#}", ended.expect("it must stop").unwrap_err())
            .contains("cannot reach the Docker engine")
    );
}
