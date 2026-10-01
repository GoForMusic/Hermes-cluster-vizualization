use hermes_proto::value::json_from_struct;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;
use crate::state::{failed_recently, node_state};

const NOW: i64 = 1_800_000_000_000;

fn from<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn iso(ago_ms: i64) -> String {
    jiff::Timestamp::from_millisecond(NOW - ago_ms)
        .unwrap()
        .to_string()
}

fn task(id: &str, slot: i64, node: &str, desired: &str, state: &str) -> SwarmTask {
    from(
        json!({"ID": format!("{id}0000000000000000"), "ServiceID": "svc-web", "NodeID": node, "Slot": slot, "DesiredState": desired, "Status": {"State": state}}),
    )
}

fn failed_ago(id: &str, ago_ms: i64) -> SwarmTask {
    let mut t = task(id, 1, "w1", "shutdown", "failed");
    t.status.timestamp = iso(ago_ms);
    t
}

fn info() -> EngineInfo {
    from(
        json!({"ServerVersion": "27.3.1", "Swarm": {"NodeID": "m1", "ControlAvailable": true, "Cluster": {"ID": "swarm-uid"}}}),
    )
}

fn nodes() -> Vec<SwarmNode> {
    vec![
        from(
            json!({"ID": "m1", "Spec": {"Role": "manager"}, "Status": {"State": "ready", "Addr": "10.0.0.1"}, "Description": {"Hostname": "mgr", "Platform": {"OS": "linux", "Architecture": "x86_64"}, "Resources": {"NanoCPUs": 2000000000i64, "MemoryBytes": 4294967296i64}, "Engine": {"EngineVersion": "27.3.1"}}}),
        ),
        from(
            json!({"ID": "w1", "Spec": {"Role": "worker"}, "Status": {"State": "ready"}, "Description": {"Hostname": "wrk"}}),
        ),
    ]
}

fn services() -> Vec<SwarmService> {
    vec![from(
        json!({"ID": "svc-web", "Spec": {"Name": "web", "Labels": {"com.docker.stack.namespace": "shop"}, "Mode": {"Replicated": {"Replicas": 2}}}}),
    )]
}

fn build_with(tasks: &[SwarmTask]) -> (Vec<PbNode>, Vec<Edge>) {
    build(&Inputs {
        source_id: "s1",
        source_name: "lab",
        info: &info(),
        nodes: &nodes(),
        services: &services(),
        tasks,
        networks: &[],
        now_ms: NOW,
    })
}

fn workload(tasks: &[SwarmTask]) -> PbNode {
    build_with(tasks)
        .0
        .into_iter()
        .find(|n| n.name == "web.1")
        .expect("no web.1")
}

fn restarts(n: &PbNode) -> Value {
    json_from_struct(n.meta.as_ref().unwrap())["restarts"].clone()
}

#[test]
fn nodes_and_running_tasks_become_hosts_and_workloads_with_a_control_link() {
    let (out, edges) = build_with(&[
        task("a", 1, "m1", "running", "running"),
        task("b", 2, "w1", "running", "running"),
    ]);
    assert_eq!(
        out.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        [
            "s1",
            "s1:n:m1",
            "s1:n:w1",
            "s1:t:a0000000000000000",
            "s1:t:b0000000000000000"
        ]
    );
    let wl: HashMap<_, _> = out
        .iter()
        .filter(|n| n.kind() == NodeKind::Workload)
        .map(|n| (n.name.as_str(), n))
        .collect();
    assert!(wl.contains_key("web.1") && wl.contains_key("web.2") && wl["web.1"].own() == Own::Ok);
    assert_eq!(edges.len(), 1);
    assert_eq!(
        (edges[0].id.as_str(), edges[0].r#type()),
        ("s1:n:m1>s1:n:w1", EdgeType::Control)
    );
    assert_eq!(
        json_from_struct(out[0].meta.as_ref().unwrap()),
        json!({"version": "Docker 27.3.1", "api": "docker.sock", "uid": "swarm-uid"})
    );
    let host = json_from_struct(out[1].meta.as_ref().unwrap());
    assert_eq!(
        host,
        json!({"ip": "10.0.0.1", "role": "manager", "osType": "linux", "arch": "amd64", "vcpu": 2, "ram": 4, "os": "linux/x86_64 · Docker 27.3.1"})
    );
}

fn network(id: &str, name: &str, extra: Value) -> SwarmNetwork {
    let mut v = json!({"Id": id, "Name": name, "Driver": "overlay", "Scope": "swarm", "IPAM": {"Config": [{"Subnet": "10.0.2.0/24"}]}});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    from(v)
}

fn on(mut t: SwarmTask, networks: &[&str]) -> SwarmTask {
    t.networks = Some(
        networks
            .iter()
            .map(|id| from(json!({"Network": {"ID": id}})))
            .collect(),
    );
    t
}

fn build_networks(tasks: &[SwarmTask], networks: &[SwarmNetwork]) -> (Vec<PbNode>, Vec<Edge>) {
    build(&Inputs {
        source_id: "s1",
        source_name: "lab",
        info: &info(),
        nodes: &nodes(),
        services: &services(),
        tasks,
        networks,
        now_ms: NOW,
    })
}

#[test]
fn an_overlay_network_is_a_node_with_a_route_to_each_workload_on_it() {
    let (out, edges) = build_networks(
        &[
            on(task("a", 1, "m1", "running", "running"), &["n1"]),
            on(task("b", 2, "w1", "running", "running"), &["n1"]),
        ],
        &[network(
            "n1",
            "shop_back",
            json!({"Options": {"encrypted": ""}, "Labels": {"com.docker.stack.namespace": "shop"}}),
        )],
    );
    let net = out.iter().find(|n| n.kind() == NodeKind::Network).unwrap();
    assert_eq!(
        (net.id.as_str(), net.name.as_str(), net.parent.as_deref()),
        ("s1:net:n1", "shop_back", Some("s1"))
    );
    assert_eq!(
        json_from_struct(net.meta.as_ref().unwrap()),
        json!({"type": "Network (overlay)", "netKind": "overlay", "subnet": "10.0.2.0/24", "ns": "shop", "internal": false, "encrypted": true, "members": 2})
    );
    let routes: Vec<_> = edges
        .iter()
        .filter(|e| e.r#type() == EdgeType::Route)
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect();
    assert_eq!(
        routes,
        [
            ("s1:net:n1", "s1:t:a0000000000000000"),
            ("s1:net:n1", "s1:t:b0000000000000000")
        ]
    );
}

#[test]
fn the_routing_mesh_local_networks_and_empty_networks_are_not_drawn() {
    let (out, edges) = build_networks(
        &[on(
            task("a", 1, "m1", "running", "running"),
            &["mesh", "br", "n1"],
        )],
        &[
            network("mesh", "ingress", json!({"Ingress": true})),
            network(
                "br",
                "bridge",
                json!({"Driver": "bridge", "Scope": "local"}),
            ),
            network("n1", "used", json!({})),
            network("n2", "nobody_is_on_it", json!({})),
        ],
    );
    let names: Vec<_> = out
        .iter()
        .filter(|n| n.kind() == NodeKind::Network)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(names, ["used"]);
    assert_eq!(
        edges
            .iter()
            .filter(|e| e.r#type() == EdgeType::Route)
            .count(),
        1
    );
}

#[test]
fn a_macvlan_network_says_what_it_is() {
    let (out, _) = build_networks(
        &[on(task("a", 1, "m1", "running", "running"), &["n1"])],
        &[network(
            "n1",
            "lan",
            json!({"Driver": "macvlan", "Internal": true}),
        )],
    );
    let net = out.iter().find(|n| n.kind() == NodeKind::Network).unwrap();
    let m = json_from_struct(net.meta.as_ref().unwrap());
    assert_eq!(
        (&m["netKind"], &m["internal"], &m["ns"]),
        (&json!("macvlan"), &json!(true), &json!("—"))
    );
}

#[test]
fn a_task_says_which_stack_it_belongs_to_and_what_image_it_runs_without_the_digest() {
    let mut t = task("a", 1, "w1", "running", "running");
    t.spec.container_spec.image = "nginx:alpine@sha256:abc".into();
    t.status.container_status.container_id = "0123456789abcdef".into();
    let m = json_from_struct(workload(&[t]).meta.as_ref().unwrap());
    assert_eq!(
        (&m["ns"], &m["image"], &m["type"]),
        (&json!("shop"), &json!("nginx:alpine"), &json!("Task"))
    );
    assert_eq!(m["containers"][0]["name"], "0123456789ab");
}

#[test]
fn a_global_service_has_one_task_per_node_named_after_the_node() {
    let global: SwarmService =
        from(json!({"ID": "svc-web", "Spec": {"Name": "agent", "Mode": {"Global": {}}}}));
    let tasks = [
        task("a", 0, "m1", "running", "running"),
        task("b", 0, "w1", "running", "running"),
    ];
    let (out, _) = build(&Inputs {
        source_id: "s1",
        source_name: "lab",
        info: &info(),
        nodes: &nodes(),
        services: &[global],
        tasks: &tasks,
        networks: &[],
        now_ms: NOW,
    });
    let names: Vec<_> = out
        .iter()
        .filter(|n| n.kind() == NodeKind::Workload)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(names, ["agent.mgr", "agent.wrk"]);
    assert_eq!(
        json_from_struct(out[3].meta.as_ref().unwrap())["type"],
        "Task (global)"
    );
    assert_eq!(
        json_from_struct(out[3].meta.as_ref().unwrap())["ns"],
        "—",
        "no stack label"
    );
}

#[test]
fn a_service_that_keeps_failing_is_a_crash_loop_with_its_last_error() {
    let mut a = failed_ago("a", 60_000);
    a.status.err = "task: non-zero exit (1)".into();
    let n = workload(&[
        a,
        failed_ago("b", 50_000),
        failed_ago("c", 40_000),
        task("new", 1, "w1", "ready", "ready"),
    ]);
    assert_eq!(
        (n.own(), n.reason.as_str(), restarts(&n)),
        (Own::Crit, "CrashLoop: task: non-zero exit (1)", json!(3))
    );
}

#[test]
fn one_failure_only_warns() {
    let n = workload(&[
        failed_ago("old", 60_000),
        task("new", 1, "w1", "ready", "ready"),
    ]);
    assert_eq!((n.own(), restarts(&n)), (Own::Warn, json!(1)));
    assert!(n.reason.starts_with("Restarting"), "{}", n.reason);
}

#[test]
fn two_recent_failures_are_worth_a_look_and_not_an_outage() {
    let n = workload(&[
        failed_ago("a", 120_000),
        failed_ago("b", 60_000),
        task("new", 1, "w1", "running", "starting"),
    ]);
    assert_eq!((n.own(), restarts(&n)), (Own::Warn, json!(2)));
}

// A service updated an hour ago leaves failed tasks in Swarm's history. They say nothing about now: the new task starting up must not be
// called a crash loop because of them.
#[test]
fn old_failures_are_forgotten() {
    let hour = 3_600_000;
    let n = workload(&[
        failed_ago("a", 90 * 60_000),
        failed_ago("b", hour + 600_000),
        failed_ago("c", 3 * hour),
        task("new", 1, "w1", "running", "preparing"),
    ]);
    assert_eq!(
        (n.own(), n.reason.as_str(), restarts(&n)),
        (Own::Warn, "Preparing", json!(0))
    );
}

#[test]
fn a_running_task_is_ok_whatever_its_history_says() {
    let n = workload(&[
        failed_ago("a", 30_000),
        failed_ago("b", 20_000),
        failed_ago("c", 10_000),
        task("new", 1, "w1", "running", "running"),
    ]);
    assert_eq!(n.own(), Own::Ok);
}

#[test]
fn a_failure_with_no_readable_time_counts_rather_than_hiding_a_crash_loop() {
    let unreadable = task("a", 1, "w1", "shutdown", "failed");
    assert!(failed_recently(&unreadable, NOW));
    let mut old = task("b", 1, "w1", "shutdown", "failed");
    old.updated_at = iso(3_600_000);
    assert!(
        !failed_recently(&old, NOW),
        "the update time is used when the status has none"
    );
}

#[test]
fn a_task_on_a_node_that_is_not_known_yet_is_left_out_and_so_is_one_of_an_unknown_service() {
    let mut orphan = task("x", 3, "w1", "running", "running");
    orphan.service_id = "svc-gone".into();
    let (out, _) = build_with(&[task("a", 1, "nowhere", "running", "running"), orphan]);
    assert!(out.iter().all(|n| n.kind() != NodeKind::Workload));
}

#[test]
fn nodes_say_how_they_are() {
    let node = |v: Value| -> SwarmNode { from(v) };
    assert_eq!(
        node_state(&node(json!({"Status": {"State": "ready"}}))).0,
        Own::Ok
    );
    assert_eq!(
        node_state(&node(
            json!({"Status": {"State": "ready"}, "Spec": {"Availability": "drain"}})
        )),
        (Own::Warn, "Drain".into())
    );
    assert_eq!(
        node_state(&node(
            json!({"Status": {"State": "ready"}, "Spec": {"Availability": "pause"}})
        )),
        (Own::Warn, "Paused".into())
    );
    assert_eq!(
        node_state(&node(json!({"Status": {"State": "down"}}))),
        (Own::Crit, "NotReady".into())
    );
    assert_eq!(
        node_state(&node(
            json!({"Status": {"State": "ready"}, "ManagerStatus": {"Reachability": "unreachable"}})
        )),
        (Own::Crit, "Unreachable manager".into())
    );
}

#[test]
fn names_are_normalised() {
    assert_eq!(
        ["x86_64", "aarch64", "amd64", "ARM64"].map(normalize_arch),
        ["amd64", "arm64", "amd64", "arm64"]
    );
    assert_eq!(short_image("nginx:alpine@sha256:abc"), "nginx:alpine");
    assert_eq!(short_image("nginx"), "nginx");
}
