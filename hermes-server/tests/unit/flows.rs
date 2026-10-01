use serde_json::{Value, json};

use super::*;

fn node(id: &str, kind: &str, meta: Value) -> Node {
    Node {
        id: id.into(),
        kind: kind.into(),
        name: id.into(),
        meta: meta.as_object().cloned().unwrap_or_default(),
        ..Default::default()
    }
}

fn route(from: &str, to: &str) -> Edge {
    Edge {
        id: format!("{from}>{to}"),
        from: from.into(),
        to: to.into(),
        kind: "route".into(),
        ..Default::default()
    }
}

fn flow(src: &str, dst: &str, served_by: &str, out: f64, back: f64) -> pb::Flow {
    pb::Flow {
        src: src.into(),
        dst: dst.into(),
        served_by: served_by.into(),
        port: 80,
        proto: "tcp".into(),
        out_mbps: out,
        in_mbps: back,
    }
}

/// s1: an Ingress-less cluster with a load balancer, `web` (Service 10.96.0.9) in front of two pods, and a load generator pod.
fn cluster() -> (Vec<Node>, Vec<Edge>) {
    let nodes = vec![
        node("s1", "cluster", json!({})),
        node(
            "s1:svc:demo:web",
            "network",
            json!({"netKind": "service", "ip": "10.96.0.9"}),
        ),
        node(
            "s1:svc:demo:lb",
            "network",
            json!({"netKind": "loadbalancer", "ip": "10.96.0.10"}),
        ),
        node("s1:outside", "network", json!({"netKind": "outside"})),
        node(
            "s1:p:demo:web-1",
            "workload",
            json!({"podIP": "10.244.0.5"}),
        ),
        node(
            "s1:p:demo:web-2",
            "workload",
            json!({"podIP": "10.244.0.6"}),
        ),
        node("s1:p:demo:load", "workload", json!({"podIP": "10.244.0.9"})),
        node("s1:n:w1", "host", json!({"ip": "192.168.1.11"})),
    ];
    let edges = vec![
        route("s1:svc:demo:web", "s1:p:demo:web-1"),
        route("s1:svc:demo:web", "s1:p:demo:web-2"),
        route("s1:svc:demo:lb", "s1:p:demo:web-1"),
        route("s1:svc:demo:lb", "s1:p:demo:web-2"),
        route("s1:outside", "s1:svc:demo:lb"),
    ];
    (nodes, edges)
}

fn rates(reports: &[&[pb::Flow]]) -> std::collections::BTreeMap<String, f64> {
    let (nodes, edges) = cluster();
    edge_rates(&nodes, &edges, reports).into_iter().collect()
}

#[test]
fn a_call_to_a_service_puts_its_rate_on_the_link_to_the_pod_that_answered() {
    let r = rates(&[&[flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 9.0)]]);
    assert_eq!(
        r.get("s1:svc:demo:web>s1:p:demo:web-1"),
        Some(&10.0),
        "both directions: it is what moved on that path"
    );
    assert_eq!(
        r.len(),
        1,
        "the other pod, and the other Service, got nothing: {r:?}"
    );
}

#[test]
fn several_callers_of_the_same_pod_add_up() {
    let r = rates(&[&[
        flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 1.0),
        flow("10.244.0.7", "10.96.0.9", "10.244.0.5", 2.0, 2.0),
    ]]);
    assert_eq!(r["s1:svc:demo:web>s1:p:demo:web-1"], 6.0);
}

#[test]
fn a_connection_seen_by_two_nodes_is_counted_once() {
    let same = flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 3.0);
    let r = rates(&[std::slice::from_ref(&same), std::slice::from_ref(&same)]);
    assert_eq!(r["s1:svc:demo:web>s1:p:demo:web-1"], 4.0);
}

#[test]
fn an_address_the_cluster_does_not_know_calling_a_load_balancer_is_outside_traffic() {
    let r = rates(&[&[flow("203.0.113.7", "10.96.0.10", "10.244.0.6", 0.5, 4.5)]]);
    assert_eq!(r["s1:outside>s1:svc:demo:lb"], 5.0);
    assert_eq!(r["s1:svc:demo:lb>s1:p:demo:web-2"], 5.0);
}

#[test]
fn a_node_port_is_found_by_the_pod_that_answered() {
    // the client asked the node's address; the Service is only known by the pod behind it
    let r = rates(&[&[flow("203.0.113.7", "192.168.1.11", "10.244.0.5", 1.0, 1.0)]]);
    assert_eq!(r["s1:outside>s1:svc:demo:lb"], 2.0);
}

#[test]
fn a_flow_that_matches_no_link_puts_a_rate_nowhere() {
    let r = rates(&[&[
        flow("10.244.0.9", "10.244.0.5", "", 1.0, 1.0),
        flow("10.244.0.9", "8.8.8.8", "", 1.0, 1.0),
        flow("10.244.0.9", "10.96.0.9", "10.244.0.77", 1.0, 1.0),
    ]]);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn a_cluster_without_an_outside_puts_nothing_on_it_but_the_service_still_carries_the_traffic() {
    let (nodes, edges) = cluster();
    let nodes: Vec<Node> = nodes.into_iter().filter(|n| n.id != "s1:outside").collect();
    let edges: Vec<Edge> = edges
        .into_iter()
        .filter(|e| e.from != "s1:outside")
        .collect();
    let r = edge_rates(
        &nodes,
        &edges,
        &[&[flow("203.0.113.7", "10.96.0.10", "10.244.0.6", 1.0, 1.0)]],
    );
    assert!(r.keys().all(|id| !id.contains("outside")), "{r:?}");
    assert_eq!(
        r["s1:svc:demo:lb>s1:p:demo:web-2"], 2.0,
        "the Service still carries it to the pod"
    );
}

/// s1 with an ingress controller (Traefik, 10.244.0.20) and an Ingress `site` that sends to the Service `web`; outside reaches the Ingress.
fn cluster_with_ingress() -> (Vec<Node>, Vec<Edge>) {
    let (mut nodes, mut edges) = cluster();
    nodes.push(node(
        "s1:p:traefik:t-1",
        "workload",
        json!({"podIP": "10.244.0.20", "ingressController": true}),
    ));
    nodes.push(node(
        "s1:ing:demo:site",
        "network",
        json!({"netKind": "ingress"}),
    ));
    edges.push(route("s1:outside", "s1:ing:demo:site"));
    edges.push(route("s1:ing:demo:site", "s1:svc:demo:web"));
    (nodes, edges)
}

#[test]
fn what_the_controller_exchanges_with_the_pods_behind_a_service_is_the_traffic_of_its_ingress() {
    let (nodes, edges) = cluster_with_ingress();
    // the controller calls a pod directly (that is what controllers do), not the Service address
    let r: std::collections::BTreeMap<_, _> = edge_rates(
        &nodes,
        &edges,
        &[&[
            flow("10.244.0.20", "10.244.0.5", "", 1.0, 5.0),
            flow("10.244.0.20", "10.244.0.6", "", 0.5, 2.5),
        ]],
    )
    .into_iter()
    .collect();
    assert_eq!(r["s1:ing:demo:site>s1:svc:demo:web"], 9.0);
    assert_eq!(
        r["s1:outside>s1:ing:demo:site"], 9.0,
        "the same amount came in from outside"
    );
    assert!(
        !r.contains_key("s1:svc:demo:lb>s1:p:demo:web-1"),
        "the load balancer is not what carried it: only the Ingress's Service is"
    );
}

#[test]
fn a_pod_that_is_not_an_ingress_controller_puts_nothing_on_an_ingress() {
    let (nodes, edges) = cluster_with_ingress();
    let r = edge_rates(
        &nodes,
        &edges,
        &[&[flow("10.244.0.9", "10.244.0.5", "", 1.0, 1.0)]],
    );
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn a_cluster_without_a_controller_or_an_ingress_is_left_alone() {
    let (nodes, edges) = cluster();
    let r = edge_rates(
        &nodes,
        &edges,
        &[&[flow("10.244.0.20", "10.244.0.5", "", 1.0, 1.0)]],
    );
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn the_busiest_connections_come_first_with_the_ends_named_by_id_and_strangers_by_address() {
    let (nodes, _) = cluster();
    let lines = top_flows(
        &nodes,
        &[&[
            flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 9.0),
            flow("203.0.113.7", "192.168.1.11", "10.244.0.6", 0.2, 0.3),
            flow("10.244.0.9", "10.244.0.6", "", 0.001, 0.001),
        ]],
        5,
    );
    assert_eq!(lines.len(), 2, "what moved almost nothing is left out");
    assert_eq!(
        (
            lines[0].src.as_str(),
            lines[0].dst.as_str(),
            lines[0].via.as_str(),
            lines[0].mbps,
            lines[0].external
        ),
        (
            "s1:p:demo:load",
            "s1:svc:demo:web",
            "s1:p:demo:web-1",
            10.0,
            false
        )
    );
    assert_eq!(
        (
            lines[1].src.as_str(),
            lines[1].dst.as_str(),
            lines[1].external
        ),
        ("203.0.113.7", "s1:n:w1", true)
    );
}

#[test]
fn the_list_is_cut_to_the_limit_and_a_connection_seen_by_two_nodes_is_listed_once() {
    let (nodes, _) = cluster();
    // twenty different callers, each with its own rate
    let many: Vec<pb::Flow> = (0..20)
        .map(|i| {
            flow(
                &format!("203.0.113.{i}"),
                "10.96.0.9",
                "10.244.0.5",
                f64::from(i) + 1.0,
                0.0,
            )
        })
        .collect();
    let lines = top_flows(&nodes, &[&many, &many], 8);
    assert_eq!(lines.len(), 8);
    assert!(lines.windows(2).all(|w| w[0].mbps >= w[1].mbps));
    assert_eq!(lines[0].mbps, 20.0, "twice reported, once counted");
}

#[test]
fn the_pods_that_answered_one_caller_are_one_line() {
    let (nodes, _) = cluster();
    let lines = top_flows(
        &nodes,
        &[&[
            flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 3.0),
            flow("10.244.0.9", "10.96.0.9", "10.244.0.6", 2.0, 4.0),
            flow("10.244.0.9", "10.244.0.6", "", 0.5, 0.5),
        ]],
        5,
    );
    assert_eq!(
        lines.len(),
        2,
        "loadgen -> the Service, and loadgen -> a pod directly"
    );
    assert_eq!(
        (lines[0].dst.as_str(), lines[0].mbps, lines[0].via.as_str()),
        ("s1:svc:demo:web", 10.0, ""),
        "two pods answered: none is named"
    );
    let one = top_flows(
        &nodes,
        &[&[flow("10.244.0.9", "10.96.0.9", "10.244.0.5", 1.0, 1.0)]],
        5,
    );
    assert_eq!(
        one[0].via, "s1:p:demo:web-1",
        "one pod answered: it is named"
    );
}
