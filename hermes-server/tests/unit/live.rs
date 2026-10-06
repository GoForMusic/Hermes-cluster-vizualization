use tokio::sync::broadcast::error::TryRecvError;

use super::*;

fn node(id: &str, kind: &str, parent: Option<&str>) -> Node {
    Node {
        id: id.into(),
        kind: kind.into(),
        name: id.into(),
        parent: parent.map(Into::into),
        own: "ok".into(),
        status: "ok".into(),
        ..Default::default()
    }
}

fn edge(id: &str) -> Edge {
    Edge {
        id: id.into(),
        from: "a".into(),
        to: "b".into(),
        kind: "traffic".into(),
        ..Default::default()
    }
}

/// The next event; it must be one the web app knows (`HubEvent`).
fn next(rx: &mut broadcast::Receiver<Arc<str>>) -> Value {
    let raw = rx.try_recv().expect("an event was expected");
    serde_json::from_str::<crate::model::HubEvent>(&raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
    serde_json::from_str(&raw).unwrap()
}

fn source(id: &str) -> Source {
    Source {
        id: id.into(),
        name: format!("name of {id}"),
        kind: "Docker Swarm (agent)".into(),
        ..Default::default()
    }
}

#[test]
fn the_snapshot_merges_the_sources_in_the_order_they_first_appeared() {
    let s = StoreImp::new();
    s.set_topology("b", vec![node("b1", "cluster", None)], vec![edge("e1")]);
    s.set_topology("a", vec![node("a1", "cluster", None)], vec![]);
    s.set_topology("b", vec![node("b2", "cluster", None)], vec![]); // replacing keeps b first
    let snap: Value = serde_json::from_str(&s.snapshot_json()).unwrap();
    let ids: Vec<_> = snap["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        (snap["type"].as_str(), ids),
        (Some("snapshot"), vec!["b2", "a1"])
    );
    assert!(s.has_source("a") && !s.has_source("c"));
}

#[test]
fn every_change_reaches_the_browsers_in_the_shape_they_know() {
    let s = StoreImp::new();
    s.set_topology(
        "s",
        vec![node("h", "host", None), node("w", "workload", Some("h"))],
        vec![edge("e")],
    );
    let mut rx = s.subscribe();

    s.set_status("w", "crit", "OOMKilled");
    assert_eq!(
        next(&mut rx),
        json!({"type":"status","id":"w","own":"crit","reason":"OOMKilled"})
    );

    s.set_meta("w", serde_json::from_value(json!({"restarts": 3})).unwrap());
    assert_eq!(
        next(&mut rx),
        json!({"type":"meta","id":"w","meta":{"restarts":3}})
    );

    let nodes = HashMap::from([("w".to_string(), HashMap::from([("cpu".to_string(), 12.5)]))]);
    s.apply_metrics(&nodes, &HashMap::from([("e".to_string(), 7.0)]));
    assert_eq!(
        next(&mut rx),
        json!({"type":"metrics","nodes":{"w":{"cpu":12.5}},"edges":{"e":7.0}})
    );

    let w = s.nodes().into_iter().find(|n| n.id == "w").unwrap();
    assert_eq!(
        (
            w.own.as_str(),
            w.reason.as_str(),
            w.m["cpu"],
            w.meta["restarts"].as_i64()
        ),
        ("crit", "OOMKilled", 12.5, Some(3))
    );
    assert_eq!(s.edges()[0].mbps, 7.0);

    s.publish(&json!({"type": "sources"}));
    assert_eq!(next(&mut rx), json!({"type":"sources"}));
}

#[test]
fn changes_to_unknown_nodes_are_ignored_quietly() {
    let s = StoreImp::new();
    let mut rx = s.subscribe();
    s.set_status("nope", "crit", "");
    s.set_meta("nope", Meta::new());
    s.set_node_stale("nope", true);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[test]
fn a_stale_source_is_marked_and_a_fresh_snapshot_clears_it() {
    let s = StoreImp::new();
    s.set_topology("s", vec![node("h", "host", None)], vec![]);
    let mut rx = s.subscribe();
    s.set_stale("s", true);
    assert_eq!(next(&mut rx)["nodes"][0]["stale"], json!(true));
    s.set_stale("s", true); // nothing changed: nothing is sent
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    s.set_topology("s", vec![node("h", "host", None)], vec![]);
    assert!(next(&mut rx)["nodes"][0].get("stale").is_none());
}

#[test]
fn a_single_node_can_be_stale_on_its_own() {
    let s = StoreImp::new();
    s.set_topology(
        "s",
        vec![node("h1", "host", None), node("h2", "host", None)],
        vec![],
    );
    s.set_node_stale("h2", true);
    let stale: Vec<_> = s.nodes().into_iter().map(|n| n.stale).collect();
    assert_eq!(stale, [false, true]);
}

#[test]
fn a_placeholder_shows_a_source_with_no_data_and_is_replaced_by_the_real_thing() {
    let s = StoreImp::new();
    s.ensure_placeholder(&source("s1"));
    let nodes = s.nodes();
    assert_eq!(
        (
            nodes.len(),
            nodes[0].kind.as_str(),
            nodes[0].provider.as_str(),
            nodes[0].stale
        ),
        (1, "cluster", "swarm", true)
    );
    assert_eq!(nodes[0].meta["version"], json!("—"));

    s.set_topology("s1", vec![node("real", "cluster", None)], vec![]);
    s.ensure_placeholder(&source("s1")); // it has a topology now: nothing to do
    assert_eq!(s.nodes()[0].id, "real");
}

#[test]
fn removing_a_source_removes_what_it_contributed() {
    let s = StoreImp::new();
    s.set_topology("a", vec![node("a1", "cluster", None)], vec![]);
    s.set_topology("b", vec![node("b1", "cluster", None)], vec![]);
    s.remove_source("a");
    assert_eq!(
        s.nodes().iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["b1"]
    );
    s.set_status("a1", "crit", ""); // its index is gone too
    assert!(!s.has_source("a"));
}

fn ids(s: &StoreImp) -> Vec<String> {
    s.nodes().into_iter().map(|n| n.id).collect()
}

#[test]
fn what_a_host_contributes_joins_the_topology_and_survives_a_new_topology_of_the_source() {
    let s = StoreImp::new();
    s.set_topology(
        "s",
        vec![node("h1", "host", None), node("h2", "host", None)],
        vec![],
    );
    s.set_contribution("s", "h2", vec![node("vol", "volume", Some("h2"))]);
    assert_eq!(ids(&s), ["h1", "h2", "vol"]);
    let snap: Value = serde_json::from_str(&s.snapshot_json()).unwrap();
    assert_eq!(
        snap["nodes"][2]["id"], "vol",
        "the browsers see it in the snapshot"
    );

    // the agent that describes the cluster sends its topology again: what the other host contributed stays
    s.set_topology(
        "s",
        vec![node("h1", "host", None), node("h2", "host", None)],
        vec![],
    );
    assert_eq!(ids(&s), ["h1", "h2", "vol"]);
}

#[test]
fn a_contribution_replaces_the_previous_one_of_the_same_host_and_leaves_the_others_alone() {
    let s = StoreImp::new();
    s.set_topology(
        "s",
        vec![node("h1", "host", None), node("h2", "host", None)],
        vec![],
    );
    s.set_contribution("s", "h1", vec![node("a", "volume", Some("h1"))]);
    s.set_contribution("s", "h2", vec![node("b", "volume", Some("h2"))]);
    s.set_contribution("s", "h1", vec![node("c", "volume", Some("h1"))]);
    assert_eq!(
        ids(&s),
        ["h1", "h2", "c", "b"],
        "h1's new list, h2's untouched"
    );
    s.set_contribution("s", "h1", vec![]);
    assert_eq!(
        ids(&s),
        ["h1", "h2", "b"],
        "an empty contribution takes them away"
    );
}

#[test]
fn a_contribution_that_comes_before_the_topology_waits_for_it() {
    let s = StoreImp::new();
    s.set_contribution("s", "h1", vec![node("vol", "volume", Some("h1"))]);
    assert!(
        !s.has_source("s"),
        "a contribution is not a topology: the agents are still asked for theirs"
    );
    assert!(ids(&s).is_empty());
    s.set_topology("s", vec![node("h1", "host", None)], vec![]);
    assert_eq!(ids(&s), ["h1", "vol"]);
}

#[test]
fn contributed_nodes_take_metrics_and_go_stale_with_their_source() {
    let s = StoreImp::new();
    s.set_topology("s", vec![node("h1", "host", None)], vec![]);
    s.set_contribution("s", "h1", vec![node("vol", "volume", Some("h1"))]);
    s.apply_metrics(
        &HashMap::from([(
            "vol".to_string(),
            HashMap::from([("used".to_string(), 1.5)]),
        )]),
        &HashMap::new(),
    );
    assert_eq!(s.nodes()[1].m["used"], 1.5);
    s.set_stale("s", true);
    assert!(s.nodes().iter().all(|n| n.stale));
    s.set_contribution(
        "s",
        "h1",
        vec![
            node("vol", "volume", Some("h1")),
            node("vol2", "volume", Some("h1")),
        ],
    );
    assert!(
        s.nodes().iter().all(|n| n.stale),
        "a new contribution of a source that cannot be reached is out of date too"
    );
}

#[test]
fn removing_a_source_removes_its_contributions_and_those_that_were_waiting() {
    let s = StoreImp::new();
    s.set_topology("s", vec![node("h1", "host", None)], vec![]);
    s.set_contribution("s", "h1", vec![node("vol", "volume", Some("h1"))]);
    s.set_contribution("other", "h", vec![node("x", "volume", None)]);
    s.remove_source("s");
    s.remove_source("other");
    s.set_topology("other", vec![node("h", "host", None)], vec![]);
    assert_eq!(ids(&s), ["h"], "nothing left over from before");
}

#[tokio::test]
async fn a_browser_that_falls_behind_is_dropped_instead_of_holding_everything_up() {
    let s = StoreImp::new();
    let mut rx = s.subscribe();
    for _ in 0..BACKLOG + 10 {
        s.publish(&json!({"type": "x"}));
    }
    assert!(matches!(
        rx.recv().await,
        Err(broadcast::error::RecvError::Lagged(_))
    ));
}

fn link(id: &str) -> Edge {
    Edge {
        id: id.into(),
        from: "net".into(),
        to: "c".into(),
        kind: "route".into(),
        ..Default::default()
    }
}

#[test]
fn the_links_a_host_contributes_are_drawn_take_traffic_and_are_replaced_with_its_nodes() {
    let s = StoreImp::new();
    s.set_topology("s", vec![node("h1", "host", None)], vec![edge("base")]);
    s.set_contribution_with_edges(
        "s",
        "h1",
        vec![
            node("net", "network", None),
            node("c", "workload", Some("h1")),
        ],
        vec![link("net>c")],
    );
    let ids_of = |s: &StoreImp| s.edges().into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids_of(&s), ["base", "net>c"]);
    assert_eq!(s.edges_for("s").len(), 2);

    s.apply_metrics(
        &HashMap::new(),
        &HashMap::from([("net>c".to_string(), 4.5)]),
    );
    assert_eq!(
        s.edges()[1].mbps,
        4.5,
        "the rate lands on the contributed link"
    );

    // the agent that describes the cluster sends its topology again: the contributed link stays
    s.set_topology("s", vec![node("h1", "host", None)], vec![edge("base")]);
    assert_eq!(ids_of(&s), ["base", "net>c"]);

    // the host contributes again, without the network: the link goes with it
    s.set_contribution_with_edges("s", "h1", vec![node("c", "workload", Some("h1"))], vec![]);
    assert_eq!(ids_of(&s), ["base"]);
}

#[test]
fn links_that_came_before_the_topology_wait_for_it() {
    let s = StoreImp::new();
    s.set_contribution_with_edges(
        "s",
        "h1",
        vec![node("net", "network", None), node("c", "workload", None)],
        vec![link("net>c")],
    );
    assert!(s.edges().is_empty(), "no topology yet, nothing is drawn");
    s.set_topology("s", vec![node("h1", "host", None)], vec![]);
    assert_eq!(s.edges().len(), 1);
}

#[test]
fn keeping_one_contribution_drops_the_others_with_their_links() {
    let s = StoreImp::new();
    s.set_topology("s", vec![node("c", "cluster", None)], vec![]);
    s.set_contribution_with_edges(
        "s",
        "h1",
        vec![node("a", "host", Some("c"))],
        vec![link("a>x")],
    );
    s.set_contribution_with_edges(
        "s",
        "h2",
        vec![node("b", "host", Some("c"))],
        vec![link("b>x")],
    );
    assert_eq!(ids(&s), ["c", "a", "b"]);
    s.keep_only_contribution("s", "h2");
    assert_eq!(ids(&s), ["c", "b"]);
    assert_eq!(s.edges().len(), 1);
    s.keep_only_contribution("s", "h2"); // nothing else left: nothing changes
    assert_eq!(ids(&s), ["c", "b"]);
}

#[test]
fn renaming_a_cluster_changes_only_the_cluster_of_that_source_and_says_so_once() {
    let s = StoreImp::new();
    s.set_topology(
        "s",
        vec![node("s", "cluster", None), node("h", "host", Some("s"))],
        vec![],
    );
    s.set_topology("t", vec![node("t", "cluster", None)], vec![]);
    let mut rx = s.subscribe();
    s.rename_cluster("s", "new");
    let names: Vec<String> = s.nodes().into_iter().map(|n| n.name).collect();
    assert_eq!(
        names,
        ["new", "h", "t"],
        "the host and the other source keep theirs"
    );
    assert_eq!(next(&mut rx)["type"], "snapshot");
    s.rename_cluster("s", "new"); // nothing changes: nothing is sent
    assert!(rx.try_recv().is_err());
}
