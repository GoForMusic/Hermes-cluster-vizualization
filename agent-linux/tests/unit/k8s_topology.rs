use hermes_proto::value::json_from_struct;
use serde_json::json;

use super::super::kubelet::GIB;
use super::*;

fn from<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn node(name: &str, control: bool, ready: bool) -> Node {
    let labels = if control {
        json!({"node-role.kubernetes.io/control-plane": ""})
    } else {
        json!({})
    };
    from(json!({
        "metadata": {"name": name, "labels": labels},
        "status": {
            "conditions": [{"type": "Ready", "status": if ready { "True" } else { "False" }}],
            "addresses": [{"type": "Hostname", "address": name}, {"type": "InternalIP", "address": "10.0.0.5"}],
            "allocatable": {"cpu": "4", "memory": "8Gi"}, "capacity": {"cpu": "4", "memory": "8Gi"},
            "nodeInfo": {"osImage": "Fedora", "kubeletVersion": "v1.34", "operatingSystem": "linux", "architecture": "amd64", "machineID": "", "systemUUID": "", "bootID": "", "kernelVersion": "", "containerRuntimeVersion": "", "kubeProxyVersion": ""}
        }
    }))
}

fn pod(name: &str, node: &str, extra: Value) -> Pod {
    let mut v = json!({
        "metadata": {"name": name, "namespace": "default"},
        "spec": {"nodeName": node, "containers": [{"name": "web", "image": "nginx:1.27"}]},
        "status": {"phase": "Running", "containerStatuses": [{"name": "web", "image": "nginx:1.27", "imageID": "", "ready": true, "restartCount": 2, "state": {"running": {}}}]}
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    from(v)
}

fn claim(name: &str, size: &str) -> PersistentVolumeClaim {
    from(
        json!({"metadata": {"name": name, "namespace": "default"}, "spec": {"storageClassName": "fast", "resources": {"requests": {"storage": size}}}, "status": {}}),
    )
}

struct World {
    nodes: Vec<Node>,
    pods: Vec<Pod>,
    pvcs: Vec<PersistentVolumeClaim>,
    nu: HashMap<String, Usage>,
    pu: HashMap<String, Usage>,
    vols: HashMap<String, f64>,
    rates: HashMap<String, (f64, f64)>,
}

impl World {
    fn new() -> Self {
        Self {
            nodes: vec![node("w1", false, true), node("cp", true, true)],
            pods: vec![pod("web-1", "w1", json!({}))],
            pvcs: vec![],
            nu: HashMap::new(),
            pu: HashMap::new(),
            vols: HashMap::new(),
            rates: HashMap::new(),
        }
    }
    fn build(&self) -> (Vec<PbNode>, Vec<Edge>) {
        build(&Inputs {
            source_id: "s1",
            source_name: "lab",
            version: "v1.34.0",
            api: "https://10.0.0.1:6443",
            uid: "uid-1",
            now_ms: 42,
            nodes: &self.nodes,
            pods: &self.pods,
            pvcs: &self.pvcs,
            services: &[],
            slices: &[],
            ingresses: &[],
            gateways: &[],
            routes: &[],
            policies: &[],
            node_usage: &self.nu,
            pod_usage: &self.pu,
            volume_used: &self.vols,
            pod_rates: &self.rates,
        })
    }
}

fn find<'a>(nodes: &'a [PbNode], id: &str) -> &'a PbNode {
    nodes.iter().find(|n| n.id == id).unwrap_or_else(|| {
        panic!(
            "no {id} in {:?}",
            nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        )
    })
}

fn metadata(n: &PbNode) -> Value {
    json_from_struct(n.meta.as_ref().unwrap())
}

#[test]
fn a_cluster_has_its_identity_and_its_hosts_and_pods_hang_under_it() {
    let (nodes, edges) = World::new().build();
    let c = find(&nodes, "s1");
    assert_eq!(
        (c.kind(), c.name.as_str(), c.own()),
        (NodeKind::Cluster, "lab", Own::Ok)
    );
    assert_eq!(
        metadata(c),
        json!({"version": "v1.34.0", "api": "https://10.0.0.1:6443", "uid": "uid-1"})
    );
    let ids: Vec<_> = nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        ["s1", "s1:n:cp", "s1:n:w1", "s1:p:default:web-1"],
        "hosts by name, then pods"
    );
    assert_eq!(find(&nodes, "s1:n:w1").parent.as_deref(), Some("s1"));
    assert_eq!(
        find(&nodes, "s1:p:default:web-1").parent.as_deref(),
        Some("s1:n:w1")
    );
    assert_eq!(edges.len(), 1);
}

#[test]
fn a_host_says_what_the_machine_is() {
    let (nodes, _) = World::new().build();
    let h = find(&nodes, "s1:n:cp");
    assert_eq!(
        metadata(h),
        json!({"ip": "10.0.0.5", "role": "control-plane", "vcpu": 4, "ram": 8, "os": "Fedora", "kubelet": "v1.34", "osType": "linux", "arch": "amd64"})
    );
    assert_eq!(metadata(find(&nodes, "s1:n:w1"))["role"], "worker");
}

#[test]
fn a_host_has_a_location_only_when_its_labels_say_where_it_is() {
    let mut w = World::new();
    w.nodes[0] = from(
        json!({"metadata": {"name": "w1", "labels": {"topology.kubernetes.io/region": "eu", "topology.kubernetes.io/zone": "eu-a"}}, "status": {}}),
    );
    let (nodes, _) = w.build();
    assert_eq!(metadata(find(&nodes, "s1:n:w1"))["location"], "eu / eu-a");
    assert!(metadata(find(&nodes, "s1:n:cp")).get("location").is_none());
}

#[test]
fn a_node_that_is_not_ready_is_a_critical_host() {
    let mut w = World::new();
    w.nodes[0] = node("w1", false, false);
    let (nodes, _) = w.build();
    let h = find(&nodes, "s1:n:w1");
    assert_eq!((h.own(), h.reason.as_str()), (Own::Crit, "NotReady"));
}

#[test]
fn the_control_plane_is_linked_to_every_worker_and_the_links_carry_no_numbers() {
    let mut w = World::new();
    w.nodes.push(node("w2", false, true));
    let (_, edges) = w.build();
    assert_eq!(
        edges.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["s1:n:cp>s1:n:w1", "s1:n:cp>s1:n:w2"]
    );
    assert!(
        edges
            .iter()
            .all(|e| e.r#type() == EdgeType::Control && e.mbps == 0.0)
    );
}

#[test]
fn a_pod_says_what_it_runs_and_how_it_does() {
    let (nodes, _) = World::new().build();
    let p = find(&nodes, "s1:p:default:web-1");
    assert_eq!(
        (p.kind(), p.own(), p.reason.as_str()),
        (NodeKind::Workload, Own::Ok, "")
    );
    let m = metadata(p);
    assert_eq!(
        (
            &m["image"],
            &m["ns"],
            &m["restarts"],
            &m["phase"],
            &m["type"]
        ),
        (
            &json!("nginx:1.27"),
            &json!("default"),
            &json!(2),
            &json!("Running"),
            &json!("Pod")
        )
    );
    assert_eq!(
        m["containers"],
        json!([{"name": "web", "image": "nginx:1.27", "ready": true, "restarts": 2, "state": "running"}])
    );
}

#[test]
fn pods_that_are_finished_or_not_placed_or_on_an_unknown_node_are_left_out() {
    let mut w = World::new();
    w.pods = vec![
        pod("done", "w1", json!({"status": {"phase": "Succeeded"}})),
        pod("unscheduled", "", json!({})),
        pod("ghost", "gone-node", json!({})),
        pod("web-1", "w1", json!({})),
    ];
    let (nodes, _) = w.build();
    assert_eq!(
        nodes
            .iter()
            .filter(|n| n.kind() == NodeKind::Workload)
            .count(),
        1
    );
}

#[test]
fn what_a_pod_uses_is_shown_absolutely_and_as_a_share_of_its_node() {
    let mut w = World::new();
    w.pu.insert(
        "default/web-1".into(),
        Usage {
            milli: 400.0,
            bytes: 1024.0 * 1024.0 * 1024.0,
        },
    ); // of 4000m and 8 GiB
    let (nodes, _) = w.build();
    let m = &find(&nodes, "s1:p:default:web-1").m;
    assert_eq!(
        (m["cpu"], m["mem"], m["cpuMilli"], m["memMiB"]),
        (10.0, 12.5, 400.0, 1024.0)
    );
}

#[test]
fn a_host_shows_its_load_only_when_metrics_exist() {
    let mut w = World::new();
    assert!(
        find(&w.build().0, "s1:n:w1").m.is_empty(),
        "without metrics-server there is nothing, not zero"
    );
    w.nu.insert(
        "w1".into(),
        Usage {
            milli: 2000.0,
            bytes: 4.0 * 1024.0 * 1024.0 * 1024.0,
        },
    );
    let (nodes, _) = w.build();
    let m = &find(&nodes, "s1:n:w1").m;
    assert_eq!((m["cpu"], m["mem"]), (50.0, 50.0));
}

#[test]
fn network_rates_are_the_pods_own_and_a_host_network_pod_has_none() {
    let mut w = World::new();
    w.pods.push(pod("dns", "w1", json!({"spec": {"nodeName": "w1", "hostNetwork": true, "containers": [{"name": "c", "image": "i"}]}})));
    w.rates.insert("default/web-1".into(), (1.5, 0.25));
    w.rates.insert("default/dns".into(), (99.0, 99.0));
    let (nodes, _) = w.build();
    let m = &find(&nodes, "s1:p:default:web-1").m;
    assert_eq!((m["rxMbps"], m["txMbps"]), (1.5, 0.25));
    assert!(
        !find(&nodes, "s1:p:default:dns").m.contains_key("rxMbps"),
        "the kubelet reports the whole node's traffic for it"
    );
}

#[test]
fn a_volume_is_placed_on_the_host_of_the_pod_that_mounts_it_and_only_then() {
    let mut w = World::new();
    w.pods.push(pod("db-0", "w1", json!({"spec": {"nodeName": "w1", "containers": [{"name": "c", "image": "i"}], "volumes": [{"name": "data", "persistentVolumeClaim": {"claimName": "data"}}]}})));
    w.pvcs = vec![claim("data", "20Gi"), claim("orphan", "1Gi")];
    w.vols.insert("default/data".into(), 12.5);
    let (nodes, _) = w.build();
    let v = find(&nodes, "s1:v:default:data");
    assert_eq!(
        (v.kind(), v.parent.as_deref(), v.m["used"]),
        (NodeKind::Volume, Some("s1:n:w1"), 12.5)
    );
    assert_eq!(
        metadata(v),
        json!({"size": 20, "sc": "fast", "mountedBy": "db-0", "ns": "default", "usageKnown": true})
    );
    assert!(
        nodes.iter().all(|n| n.id != "s1:v:default:orphan"),
        "a claim nobody mounts has nowhere to be"
    );
}

#[test]
fn a_volume_whose_usage_is_not_known_says_so_instead_of_being_empty() {
    let mut w = World::new();
    w.pods.push(pod("db-0", "w1", json!({"spec": {"nodeName": "w1", "containers": [{"name": "c", "image": "i"}], "volumes": [{"name": "d", "persistentVolumeClaim": {"claimName": "data"}}]}})));
    w.pvcs = vec![claim("data", "20Gi")];
    let (nodes, _) = w.build();
    let v = find(&nodes, "s1:v:default:data");
    assert_eq!(
        (v.m["used"], metadata(v)["usageKnown"].clone()),
        (0.0, json!(false))
    );
}

#[test]
fn a_claim_is_as_big_as_it_was_granted_or_else_as_asked() {
    let granted: PersistentVolumeClaim = from(
        json!({"metadata": {"name": "c"}, "spec": {"resources": {"requests": {"storage": "1Gi"}}}, "status": {"capacity": {"storage": "2Gi"}}}),
    );
    assert_eq!(claim_size(&granted), 2.0 * GIB);
    assert_eq!(claim_size(&claim("c", "1Gi")), GIB);
    assert_eq!(claim_size(&from(json!({"metadata": {"name": "c"}}))), 0.0);
}
