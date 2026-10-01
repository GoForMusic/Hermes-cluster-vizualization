use std::sync::Arc;

use hermes_proto::v1::Own;
use hermes_proto::value::struct_from_json;
use serde_json::json;

use super::*;
use crate::testing::{Call, RecordingSink};

const TEST_RUNTIME_META: &[&str] = &["restarts", "usageKnown"];

fn node(id: &str, own: Own, restarts: i64) -> Node {
    Node {
        id: id.into(),
        name: id.into(),
        kind: NodeKind::Workload.into(),
        own: own.into(),
        m: [("cpu".to_string(), 1.0)].into(),
        meta: Some(struct_from_json(json!({"restarts": restarts}))),
        ..Default::default()
    }
}

fn count(sink: &RecordingSink, f: impl Fn(&Call) -> bool) -> usize {
    sink.calls().iter().filter(|c| f(c)).count()
}

#[test]
fn the_first_call_sends_a_snapshot_then_only_changes() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    assert!(d.apply(vec![node("a", Own::Ok, 0)], vec![]));
    assert_eq!(count(&sink, |c| matches!(c, Call::Topology(..))), 1);

    assert!(
        !d.apply(vec![node("a", Own::Ok, 0)], vec![]),
        "identical state: no snapshot"
    );
    assert_eq!(count(&sink, |c| matches!(c, Call::Status(..))), 0);

    d.apply(vec![node("a", Own::Crit, 0)], vec![]);
    assert_eq!(
        count(&sink, |c| matches!(c, Call::Status(..))),
        1,
        "a status change is reported"
    );

    d.apply(vec![node("a", Own::Crit, 3)], vec![]);
    let metas: Vec<_> = sink
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::Meta(..)))
        .collect();
    assert_eq!(metas.len(), 1, "a change of what is running is reported");
    let Call::Meta(id, patch) = &metas[0] else {
        unreachable!()
    };
    assert_eq!(
        (id.as_str(), hermes_proto::value::json_from_struct(patch)),
        ("a", json!({"restarts": 3}))
    );
}

#[test]
fn a_volume_that_becomes_measured_says_so() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    let volume = |known: bool| Node {
        id: "v".into(),
        name: "v".into(),
        kind: NodeKind::Volume.into(),
        meta: Some(struct_from_json(json!({"usageKnown": known}))),
        ..Default::default()
    };
    d.apply(vec![volume(false)], vec![]);
    d.apply(vec![volume(true)], vec![]);
    let Some(Call::Meta(id, patch)) = sink
        .calls()
        .into_iter()
        .find(|c| matches!(c, Call::Meta(..)))
    else {
        panic!("no meta patch")
    };
    assert_eq!(
        (id.as_str(), hermes_proto::value::json_from_struct(&patch)),
        ("v", json!({"usageKnown": true}))
    );
}

#[test]
fn every_round_sends_the_numbers_of_everything_but_the_cluster() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    let cluster = Node {
        id: "c".into(),
        name: "c".into(),
        kind: NodeKind::Cluster.into(),
        ..Default::default()
    };
    d.apply(vec![cluster.clone(), node("a", Own::Ok, 0)], vec![]);
    d.apply(vec![cluster, node("a", Own::Ok, 0)], vec![]);
    let Some(Call::Metrics(nodes, _)) = sink
        .calls()
        .into_iter()
        .find(|c| matches!(c, Call::Metrics(..)))
    else {
        panic!("no metrics")
    };
    assert_eq!(nodes.keys().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn nothing_measured_sends_no_metrics_call_at_all() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    let bare = Node {
        id: "a".into(),
        name: "a".into(),
        kind: NodeKind::Workload.into(),
        ..Default::default()
    }; // no `m`
    d.apply(vec![bare.clone()], vec![]);
    d.apply(vec![bare], vec![]); // topology unchanged, nothing to report
    assert_eq!(
        count(&sink, |c| matches!(c, Call::Metrics(..))),
        0,
        "an empty metrics map is not worth a call"
    );
}

#[test]
fn a_new_node_means_a_new_snapshot() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    d.apply(vec![node("a", Own::Ok, 0)], vec![]);
    assert!(d.apply(vec![node("a", Own::Ok, 0), node("b", Own::Ok, 0)], vec![]));
    assert_eq!(count(&sink, |c| matches!(c, Call::Topology(..))), 2);
}

#[test]
fn the_same_nodes_in_a_different_order_are_not_a_new_topology() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    d.apply(vec![node("a", Own::Ok, 0), node("b", Own::Ok, 0)], vec![]);
    assert!(
        !d.apply(vec![node("b", Own::Ok, 0), node("a", Own::Ok, 0)], vec![]),
        "the collector may not list them in the same order every round"
    );
    assert_eq!(count(&sink, |c| matches!(c, Call::Topology(..))), 1);
}

#[test]
fn a_renamed_node_means_a_new_snapshot_too() {
    let sink = Arc::new(RecordingSink::default());
    let mut d = Differ::new(sink.clone(), TEST_RUNTIME_META);
    d.apply(vec![node("a", Own::Ok, 0)], vec![]);
    let renamed = Node {
        name: "other".into(),
        ..node("a", Own::Ok, 0)
    };
    assert!(d.apply(vec![renamed], vec![]));
}
