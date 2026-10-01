//! The collector against a fake API server that answers the way a real one does, through the real `kube` client: what it asks for, how it
//! reads the answers, and what it does when metrics-server or a kubelet is not there.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use hermes_agentkit::testing::{Call, RecordingSink};
use serde_json::{Value, json};

use super::*;

fn list(kind: &str, items: Value) -> Json<Value> {
    Json(
        json!({"apiVersion": "v1", "kind": kind, "metadata": {"resourceVersion": "1"}, "items": items}),
    )
}

/// A one-node cluster with a pod that mounts a volume. `with_metrics`: metrics-server is installed.
async fn fake_api(with_metrics: bool) -> SocketAddr {
    fake_api_with(with_metrics, false).await
}

/// `with_networks`: the account may read Services, EndpointSlices and Ingresses (an agent installed earlier may not).
async fn fake_api_with(with_metrics: bool, with_networks: bool) -> SocketAddr {
    let node = json!({
        "metadata": {"name": "w1", "labels": {"node-role.kubernetes.io/control-plane": ""}},
        "status": {
            "conditions": [{"type": "Ready", "status": "True"}],
            "addresses": [{"type": "InternalIP", "address": "10.0.0.5"}],
            "allocatable": {"cpu": "4", "memory": "8Gi"}, "capacity": {"cpu": "4", "memory": "8Gi"},
            "nodeInfo": {"osImage": "Fedora", "kubeletVersion": "v1.34", "operatingSystem": "linux", "architecture": "amd64", "machineID": "", "systemUUID": "", "bootID": "", "kernelVersion": "", "containerRuntimeVersion": "", "kubeProxyVersion": ""}
        }
    });
    let pod = json!({
        "metadata": {"name": "db-0", "namespace": "default"},
        "spec": {"nodeName": "w1", "containers": [{"name": "db", "image": "postgres:17"}], "volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "data"}}]},
        "status": {"phase": "Running", "containerStatuses": [{"name": "db", "image": "postgres:17", "imageID": "", "ready": true, "restartCount": 0, "state": {"running": {}}}]}
    });
    let pvc = json!({"metadata": {"name": "data", "namespace": "default"}, "spec": {"storageClassName": "fast", "resources": {"requests": {"storage": "2Gi"}}}, "status": {}});
    let stats = json!({"pods": [{"podRef": {"name": "db-0", "namespace": "default"}, "volume": [{"usedBytes": 1073741824u64, "capacityBytes": 2000000000u64, "pvcRef": {"name": "data", "namespace": "default"}}]}]});

    let mut app = Router::new()
        .route("/version", get(|| async { Json(json!({"major": "1", "minor": "34", "gitVersion": "v1.34.1", "gitCommit": "", "gitTreeState": "", "buildDate": "", "goVersion": "", "compiler": "", "platform": ""})) }))
        .route("/api/v1/namespaces/kube-system", get(|| async { Json(json!({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": "kube-system", "uid": "uid-kube-system"}})) }))
        .route("/api/v1/nodes", get({ let n = node.clone(); move || async move { list("NodeList", json!([n])) } }))
        .route("/api/v1/pods", get({ let p = pod.clone(); move || async move { list("PodList", json!([p])) } }))
        .route("/api/v1/persistentvolumeclaims", get({ let p = pvc.clone(); move || async move { list("PersistentVolumeClaimList", json!([p])) } }))
        .route("/api/v1/nodes/{name}/proxy/stats/summary", get(move |Path(name): Path<String>| { let stats = stats.clone(); async move { if name == "w1" { Ok(Json(stats)) } else { Err(StatusCode::NOT_FOUND) } } }));
    if with_networks {
        app = app
            .route("/api/v1/services", get(|| async { list("ServiceList", json!([{"metadata": {"name": "db", "namespace": "default"}, "spec": {"type": "ClusterIP", "clusterIP": "10.96.0.9", "ports": [{"port": 5432}]}}])) }))
            .route("/apis/discovery.k8s.io/v1/endpointslices", get(|| async { list("EndpointSliceList", json!([{"metadata": {"name": "db-x", "namespace": "default", "labels": {"kubernetes.io/service-name": "db"}}, "addressType": "IPv4", "endpoints": [{"addresses": ["10.1.0.4"], "targetRef": {"kind": "Pod", "name": "db-0", "namespace": "default"}}]}])) }))
            .route("/apis/gateway.networking.k8s.io/v1/gateways", get(|| async { list("GatewayList", json!([{"apiVersion": "gateway.networking.k8s.io/v1", "kind": "Gateway", "metadata": {"name": "edge", "namespace": "default"}, "spec": {"gatewayClassName": "x", "listeners": [{"name": "web", "port": 80, "protocol": "HTTP"}]}}])) }))
            .route("/apis/gateway.networking.k8s.io/v1/httproutes", get(|| async { list("HTTPRouteList", json!([{"apiVersion": "gateway.networking.k8s.io/v1", "kind": "HTTPRoute", "metadata": {"name": "r", "namespace": "default"}, "spec": {"parentRefs": [{"name": "edge"}], "rules": [{"backendRefs": [{"name": "db", "port": 5432}]}]}}])) }))
            .route("/apis/networking.k8s.io/v1/networkpolicies", get(|| async { list("NetworkPolicyList", json!([{"metadata": {"name": "open", "namespace": "default"}, "spec": {"podSelector": {}, "ingress": [{}]}}])) }))
            .route("/apis/networking.k8s.io/v1/ingresses", get(|| async { list("IngressList", json!([{"metadata": {"name": "site", "namespace": "default"}, "spec": {"rules": [{"host": "db.example.com", "http": {"paths": [{"path": "/", "pathType": "Prefix", "backend": {"service": {"name": "db", "port": {"number": 5432}}}}]}}]}}])) }));
    }
    if with_metrics {
        app = app
            .route("/apis/metrics.k8s.io/v1beta1/nodes", get(|| async { Json(json!({"items": [{"metadata": {"name": "w1"}, "usage": {"cpu": "2", "memory": "4Gi"}}]})) }))
            .route("/apis/metrics.k8s.io/v1beta1/pods", get(|| async { Json(json!({"items": [{"metadata": {"name": "db-0", "namespace": "default"}, "containers": [{"usage": {"cpu": "400m", "memory": "1Gi"}}]}]})) }));
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(axum::serve(listener, app).into_future());
    addr
}

async fn collect(addr: SocketAddr) -> Arc<RecordingSink> {
    let sink = Arc::new(RecordingSink::default());
    let config = kube::Config::new(format!("http://{addr}").parse().unwrap());
    let shared: SharedSink = sink.clone();
    let task = tokio::spawn(async move { watch(config, "s1", "lab", shared).await });
    for _ in 0..100 {
        if sink.calls().iter().any(|c| matches!(c, Call::Report(..))) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    task.abort();
    sink
}

fn topology(sink: &RecordingSink) -> Vec<hermes_proto::v1::Node> {
    sink.calls()
        .into_iter()
        .find_map(|c| {
            if let Call::Topology(nodes, _) = c {
                Some(nodes)
            } else {
                None
            }
        })
        .expect("no topology was sent")
}

#[tokio::test]
async fn reads_a_cluster_through_the_real_client_and_reports_what_it_found() {
    let addr = fake_api(true).await;
    let sink = collect(addr).await;
    let nodes = topology(&sink);
    let ids: Vec<_> = nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        ["s1", "s1:n:w1", "s1:p:default:db-0", "s1:v:default:data"]
    );

    let cluster = hermes_proto::value::json_from_struct(nodes[0].meta.as_ref().unwrap());
    assert_eq!(
        (&cluster["version"], &cluster["uid"]),
        (&json!("v1.34.1"), &json!("uid-kube-system"))
    );
    assert!(
        cluster["api"]
            .as_str()
            .unwrap()
            .starts_with(&format!("http://{addr}")),
        "{cluster}"
    );

    let host = &nodes[1];
    assert_eq!(
        (host.m["cpu"], host.m["mem"]),
        (50.0, 50.0),
        "metrics-server: 2 of 4 cores, 4 of 8 GiB"
    );
    let pod = &nodes[2];
    assert_eq!((pod.m["cpuMilli"], pod.m["memMiB"]), (400.0, 1024.0));
    let volume = &nodes[3];
    assert_eq!(
        volume.m["used"], 1.0,
        "the kubelet said 1 GiB is used, on a filesystem of its own"
    );
    assert_eq!(
        hermes_proto::value::json_from_struct(volume.meta.as_ref().unwrap())["usageKnown"],
        json!(true)
    );

    let report = sink
        .calls()
        .into_iter()
        .find_map(|c| {
            if let Call::Report(state, info) = c {
                Some((state, info))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        report,
        (
            CollectorState::Connected,
            "1 nodes · 1 pods · 1 volumes".to_string()
        )
    );
}

#[tokio::test]
async fn a_cluster_without_metrics_server_is_still_drawn_just_without_load() {
    let sink = collect(fake_api(false).await).await;
    let nodes = topology(&sink);
    assert_eq!(nodes.len(), 4);
    assert!(
        nodes[1].m.is_empty() && nodes[2].m.is_empty(),
        "no numbers, not zeros"
    );
}

#[tokio::test]
async fn services_ingresses_and_outside_are_read_when_the_account_may_and_left_out_when_it_may_not()
{
    let sink = collect(fake_api_with(false, true).await).await;
    let names: Vec<_> = topology(&sink)
        .iter()
        .filter(|n| n.kind() == NodeKind::Network)
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(
        names,
        [
            "s1:outside",
            "s1:ing:default:site",
            "s1:gw:default:edge",
            "s1:rt:default:r",
            "s1:svc:default:db",
            "s1:np:default:open"
        ],
        "Ingress, Gateway API, Service and policy, read through the real client"
    );
    let edges = sink
        .calls()
        .into_iter()
        .find_map(|c| {
            if let Call::Topology(_, edges) = c {
                Some(edges)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e.from == "s1:svc:default:db" && e.to == "s1:p:default:db-0")
    );

    // an agent whose account cannot read them (the API answers 404/403) still draws everything else
    let sink = collect(fake_api_with(false, false).await).await;
    assert!(
        topology(&sink)
            .iter()
            .all(|n| n.kind() != NodeKind::Network)
    );
}

#[tokio::test]
async fn an_api_server_that_is_not_there_is_an_error_that_says_where() {
    let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = unused.local_addr().unwrap();
    drop(unused); // nothing listens here any more
    let sink: SharedSink = Arc::new(RecordingSink::default());
    let error = watch(
        kube::Config::new(format!("http://{addr}").parse().unwrap()),
        "s1",
        "lab",
        sink,
    )
    .await
    .unwrap_err();
    assert!(
        format!("{error:#}").contains(&addr.to_string()),
        "{error:#}"
    );
}
