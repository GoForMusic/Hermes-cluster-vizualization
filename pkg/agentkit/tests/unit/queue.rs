use hermes_proto::v1::NodeKind;
use hermes_proto::value::struct_from_json;
use serde_json::json;

use super::*;

fn node(id: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Workload.into(),
        name: id.into(),
        own: Own::Ok.into(),
        ..Default::default()
    }
}

fn kinds(events: &[Event]) -> Vec<&'static str> {
    events
        .iter()
        .map(|e| match e.kind.as_ref().unwrap() {
            event::Kind::Snapshot(_) => "snapshot",
            event::Kind::Status(_) => "status",
            event::Kind::Meta(_) => "meta",
            event::Kind::Metrics(_) => "metrics",
            event::Kind::Report(_) => "report",
            event::Kind::Alive(_) => "alive",
            event::Kind::Contribution(_) => "contribution",
            event::Kind::Flows(_) => "flows",
        })
        .collect()
}

#[test]
fn a_snapshot_supersedes_what_was_queued() {
    let mut s = State::default();
    s.status("a", Own::Crit, "x");
    s.set_topology(vec![node("a")], vec![]);
    assert_eq!(kinds(&s.take_batch()), ["snapshot"]);
    assert!(
        s.take_batch().is_empty(),
        "the queue is drained: an empty batch is the heartbeat"
    );
}

#[test]
fn only_the_newest_metrics_and_alive_are_kept() {
    let mut s = State::default();
    s.metrics(
        [("a".to_string(), [("cpu".to_string(), 1.0)].into())].into(),
        HashMap::new(),
    );
    s.metrics(
        [("a".to_string(), [("cpu".to_string(), 2.0)].into())].into(),
        HashMap::new(),
    );
    s.alive(vec!["a".into()]);
    s.alive(vec!["a".into(), "b".into()]);
    let batch = s.take_batch();
    assert_eq!(kinds(&batch), ["metrics", "alive"]);
    let Some(event::Kind::Metrics(m)) = &batch[0].kind else {
        panic!()
    };
    assert_eq!(m.nodes["a"].values["cpu"], 2.0);
    let Some(event::Kind::Alive(a)) = &batch[1].kind else {
        panic!()
    };
    assert_eq!(a.ids, ["a", "b"]);
}

#[test]
fn resync_sends_what_is_true_now_not_what_was_true_at_the_last_topology_change() {
    let mut s = State::default();
    s.set_topology(vec![node("a")], vec![]);
    s.take_batch();
    s.status("a", Own::Crit, "OOMKilled");
    s.meta("a", struct_from_json(json!({"restarts": 4})));
    s.take_batch();

    s.resync = true;
    let batch = s.take_batch();
    assert_eq!(kinds(&batch), ["snapshot"]);
    let Some(event::Kind::Snapshot(snap)) = &batch[0].kind else {
        panic!()
    };
    let a = &snap.nodes[0];
    assert_eq!((a.own(), a.reason.as_str()), (Own::Crit, "OOMKilled"));
    assert_eq!(
        a.meta.as_ref().unwrap().fields["restarts"],
        hermes_proto::value::value_from_json(json!(4))
    );
}

#[test]
fn resync_repeats_alive_and_report_once_and_without_duplicates() {
    let mut s = State::default();
    s.alive(vec!["p1".into()]);
    s.report(CollectorState::Connected, "ok");
    assert_eq!(kinds(&s.take_batch()), ["alive", "report"]);
    assert!(s.take_batch().is_empty());

    s.resync = true;
    assert_eq!(
        kinds(&s.take_batch()),
        ["alive", "report"],
        "the hub lost them"
    );

    s.resync = true;
    s.alive(vec!["p2".into()]);
    assert_eq!(
        kinds(&s.take_batch()),
        ["alive", "report"],
        "the queued alive is not sent twice"
    );
}

#[test]
fn a_contribution_replaces_the_previous_one_and_is_sent_again_after_a_resync() {
    let mut s = State::default();
    s.contribute_with_edges(vec![node("v1")], vec![]);
    s.contribute_with_edges(vec![node("v1"), node("v2")], vec![]);
    let batch = s.take_batch();
    assert_eq!(
        kinds(&batch),
        ["contribution"],
        "only the newest one is sent"
    );
    let Some(event::Kind::Contribution(c)) = &batch[0].kind else {
        panic!()
    };
    assert_eq!(c.nodes.len(), 2);

    s.resync = true;
    assert_eq!(kinds(&s.take_batch()), ["contribution"], "the hub lost it");
    s.resync = true;
    s.contribute_with_edges(vec![node("v3")], vec![]);
    assert_eq!(
        kinds(&s.take_batch()),
        ["contribution"],
        "a queued one is not sent twice"
    );
}

#[test]
fn a_report_is_sent_only_when_the_state_changes() {
    let mut s = State::default();
    s.report(CollectorState::Error, "cluster unreachable");
    s.take_batch();
    s.report(CollectorState::Error, "still unreachable");
    assert!(s.take_batch().is_empty());
    s.report(CollectorState::Connected, "back");
    assert_eq!(kinds(&s.take_batch()), ["report"]);
}

#[test]
fn the_queue_is_bounded_and_drops_the_oldest() {
    let mut s = State::default();
    for i in 0..MAX_QUEUE + 20 {
        s.status(&format!("n{i}"), Own::Ok, "");
    }
    let batch = s.take_batch();
    assert_eq!(batch.len(), MAX_QUEUE);
    let Some(event::Kind::Status(first)) = &batch[0].kind else {
        panic!()
    };
    assert_eq!(first.id, "n20");
}
