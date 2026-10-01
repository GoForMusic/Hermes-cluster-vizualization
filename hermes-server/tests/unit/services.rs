use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::database::Repositories;
use crate::model::{Node, Severity, Source};

fn node(id: &str, kind: &str, parent: Option<&str>, own: &str, meta: Value) -> Node {
    Node {
        id: id.into(),
        kind: kind.into(),
        name: id.into(),
        parent: parent.map(Into::into),
        own: own.into(),
        status: own.into(),
        meta: serde_json::from_value(meta).unwrap(),
        ..Default::default()
    }
}

fn world() -> Vec<Node> {
    vec![
        node("c", "cluster", None, "ok", json!({})),
        node("h", "host", Some("c"), "ok", json!({"role": "worker"})),
        node("w", "workload", Some("h"), "ok", json!({"restarts": 0})),
    ]
}

fn engine() -> (Arc<dyn IEngine>, Arc<dyn IStore>, Repositories) {
    let store: Arc<dyn IStore> = Arc::new(StoreImp::new());
    let db = Repositories::sqlite_in_memory().unwrap();
    let e = EngineImp::new(
        store.clone(),
        db.alerts.clone(),
        db.sources.clone(),
        db.beats.clone(),
        db.settings.clone(),
    );
    (Arc::new(e), store, db)
}

fn titles(db: &Repositories) -> Vec<String> {
    let mut t: Vec<_> = db
        .alerts
        .active_alerts()
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    t.sort();
    t
}

fn all_rules() -> HashMap<String, Rule> {
    defaults()
}

#[test]
fn a_healthy_world_wants_no_alert_and_every_node_is_up() {
    let (want, beats) = judge(&world(), &[], &all_rules());
    assert!(want.is_empty());
    assert_eq!(
        beats.iter().filter(|(_, b)| *b == Beat::Up).count(),
        2,
        "hosts and workloads have a heartbeat; the cluster does not"
    );
}

#[test]
fn a_crashed_host_raises_one_alert_and_does_not_blame_its_workloads() {
    let mut nodes = world();
    nodes[1].own = "crit".into();
    nodes[2].own = "crit".into();
    let (want, beats) = judge(&nodes, &[], &all_rules());
    assert_eq!(want.len(), 1);
    let a = &want["host:h"];
    assert_eq!(
        (a.sev, a.title.as_str(), a.detail.as_str()),
        (
            Severity::Crit,
            "Worker h unreachable",
            "c · no heartbeat · 1 workloads affected"
        )
    );
    assert!(
        beats.iter().all(|(_, b)| *b == Beat::Down),
        "the workloads are down with their host"
    );
}

#[test]
fn a_host_says_what_kind_of_machine_it_is() {
    let mut nodes = world();
    nodes[1].own = "crit".into();
    nodes[1].meta.insert("role".into(), json!("control-plane"));
    nodes[1].reason = "NodeNotReady".into();
    assert_eq!(
        judge(&nodes, &[], &all_rules()).0["host:h"].title,
        "Control-plane node h unreachable"
    );
    nodes[1].meta.insert("role".into(), json!("manager"));
    assert_eq!(
        judge(&nodes, &[], &all_rules()).0["host:h"].title,
        "Manager h unreachable"
    );
}

#[test]
fn a_failing_workload_on_a_live_host_is_an_alert_with_its_restarts() {
    let mut nodes = world();
    nodes[2].own = "crit".into();
    nodes[2].reason = "CrashLoopBackOff".into();
    nodes[2].meta.insert("restarts".into(), json!(7));
    let (want, _) = judge(&nodes, &[], &all_rules());
    assert_eq!(
        (want["wl:w"].title.as_str(), want["wl:w"].detail.as_str()),
        ("w CrashLoopBackOff", "c / h · restarts 7")
    );
}

#[test]
fn a_host_s_heartbeat_bar_goes_amber_for_a_struggling_workload_never_red_for_it() {
    // A pod stuck pending (warn) or fully crashing (crit) on an otherwise fine host should still show up on that host's own
    // heartbeat bar, not just as an alert on the pod — but capped at "warn": one bad pod is not the machine being unreachable, and
    // the workload's own alert already explains why.
    let mut nodes = world();
    nodes[2].own = "warn".into(); // w, on host h
    let beat = |b: &[(String, Beat)], id: &str| b.iter().find(|(n, _)| n == id).unwrap().1;
    assert_eq!(beat(&judge(&nodes, &[], &all_rules()).1, "h"), Beat::Warn);

    nodes[2].own = "crit".into();
    assert_eq!(
        beat(&judge(&nodes, &[], &all_rules()).1, "h"),
        Beat::Warn,
        "still amber, not down: the host's own heartbeat is fine"
    );
}

#[test]
fn a_volume_warns_at_the_threshold_and_is_critical_above_the_other() {
    let mut nodes = world();
    nodes.push(Node {
        m: HashMap::from([("used".to_string(), 90.0)]),
        ..node("v", "volume", Some("h"), "ok", json!({"size": 100}))
    });
    let (want, beats) = judge(&nodes, &[], &all_rules());
    assert_eq!(
        (
            want["vol:v"].sev,
            want["vol:v"].title.as_str(),
            want["vol:v"].detail.as_str()
        ),
        (Severity::Warn, "Volume v at 90%", "90 GiB of 100 GiB on h")
    );
    assert_eq!(
        beats.iter().find(|(id, _)| id == "v").unwrap().1,
        Beat::Warn
    );

    nodes[3].m.insert("used".into(), 96.0);
    assert_eq!(
        judge(&nodes, &[], &all_rules()).0["vol:v"].sev,
        Severity::Crit
    );

    let mut rules = all_rules();
    rules.get_mut("volume-usage").unwrap().enabled = false;
    assert!(
        judge(&nodes, &[], &rules).0.is_empty(),
        "a disabled rule raises nothing"
    );
}

#[test]
fn terraform_drift_is_a_warning_with_the_note() {
    let mut nodes = world();
    nodes[1]
        .meta
        .insert("iac".into(), json!({"drift": true, "note": "disk changed"}));
    let (want, _) = judge(&nodes, &[], &all_rules());
    assert_eq!(
        (want["drift:h"].sev, want["drift:h"].detail.as_str()),
        (Severity::Warn, "disk changed")
    );
}

#[test]
fn a_source_that_cannot_be_reached_raises_one_alert_and_nothing_underneath_it_is_trusted() {
    let mut nodes = world();
    for n in &mut nodes {
        n.stale = true;
        n.own = "crit".into();
    }
    let src = Source {
        id: "c".into(), // matches world()'s cluster node: the source this cluster belongs to
        name: "prod".into(),
        state: "error".into(),
        info: "agent silent for 25s".into(),
        ..Default::default()
    };
    let (want, beats) = judge(&nodes, &[src], &all_rules());
    assert_eq!(want.keys().collect::<Vec<_>>(), ["src:c"]);
    assert_eq!(want["src:c"].title, "Source prod unreachable");
    assert!(
        beats.iter().all(|(_, b)| *b == Beat::NoData),
        "with the source itself unreachable, even an own of \"crit\" is stale data, not a fresh finding"
    );
}

#[test]
fn a_host_down_still_alerts_in_red_even_if_its_own_agent_has_gone_quiet() {
    // A per-node agent (a Kubernetes DaemonSet, a Swarm worker's own metrics feed) can go stale on a machine that a different,
    // still-live collector (the API server, the Swarm manager) independently and freshly reports as down — e.g. the whole VM was
    // shut off. That is real, current information, not "nothing is known": it must still raise the alert and show red, not the
    // grey of a node we simply have not heard from.
    let mut nodes = world();
    nodes[1].own = "crit".into();
    nodes[1].reason = "NotReady".into();
    nodes[1].stale = true; // this host's own agent is quiet, but the source below is not unreachable
    nodes[2].stale = true;
    let src = Source {
        id: "c".into(),
        name: "prod".into(),
        state: "ok".into(),
        ..Default::default()
    };
    let (want, beats) = judge(&nodes, &[src], &all_rules());
    assert_eq!(want.keys().collect::<Vec<_>>(), ["host:h"]);
    assert!(beats.iter().all(|(_, b)| *b == Beat::Down));
}

#[test]
fn a_node_that_is_merely_quiet_with_nothing_wrong_reported_raises_no_alert() {
    let mut nodes = world();
    nodes[1].stale = true; // own is still "ok": no other collector is claiming trouble, just no recent word from this one
    let (want, beats) = judge(&nodes, &[], &all_rules());
    assert!(want.is_empty());
    assert_eq!(
        beats.iter().find(|(id, _)| id == "h").unwrap().1,
        Beat::NoData
    );
}

#[test]
fn an_alert_opens_once_updates_in_place_and_resolves_after_the_hold_down() {
    let (e, store, db) = engine();
    store.set_topology("s", world(), vec![]);
    let mut rx = store.subscribe();
    e.evaluate(1_000);
    assert!(titles(&db).is_empty());

    store.set_status("w", "crit", "CrashLoopBackOff");
    e.evaluate(2_000);
    e.evaluate(3_000);
    assert_eq!(
        titles(&db),
        ["w CrashLoopBackOff"],
        "one alert, however many times it is evaluated"
    );
    let opened = db.alerts.active_alerts().unwrap().remove(0);
    assert_eq!(opened.ts, 2_000);

    store.set_status("w", "crit", "OOMKilled");
    e.evaluate(4_000);
    assert_eq!(titles(&db), ["w OOMKilled"], "the same alert, updated");

    store.set_status("w", "ok", "");
    e.evaluate(5_000); // the condition went away: start the hold-down
    e.evaluate(5_000 + HOLD_DOWN_MS - 1);
    assert_eq!(
        db.alerts.active_alerts().unwrap().len(),
        1,
        "still waiting: a crash-looping service must not flap"
    );

    store.set_status("w", "crit", "OOMKilled");
    e.evaluate(5_000 + HOLD_DOWN_MS + 1); // it failed again in time: the hold-down starts over
    store.set_status("w", "ok", "");
    e.evaluate(30_000);
    e.evaluate(30_000 + HOLD_DOWN_MS + 1);
    assert!(db.alerts.active_alerts().unwrap().is_empty());
    let resolved = db.alerts.get_alert(opened.id).unwrap().unwrap();
    assert_eq!(
        resolved.resolved_ts,
        Some(30_000),
        "healthy from the moment the condition went away"
    );

    // the browsers were told about every step
    let mut events = Vec::new();
    while let Ok(m) = rx.try_recv() {
        let v: Value = serde_json::from_str(&m).unwrap();
        if v["type"] == "alert" {
            events.push(v["isNew"].as_bool().unwrap());
        }
    }
    assert_eq!(events.first(), Some(&true));
    assert_eq!(events.last(), Some(&false));
}

#[test]
fn a_new_alert_snapshots_the_node_it_is_about_but_never_updates_it() {
    let (e, store, db) = engine();
    store.set_topology("s", world(), vec![]);
    e.evaluate(1_000);

    store.set_status("w", "crit", "CrashLoopBackOff");
    store.set_meta("w", serde_json::from_value(json!({"restarts": 3})).unwrap());
    e.evaluate(2_000);
    let opened = db.alerts.active_alerts().unwrap().remove(0);
    let snap: Value = serde_json::from_str(&opened.snapshot).unwrap();
    assert_eq!(
        (&snap["reason"], &snap["meta"]["restarts"]),
        (&json!("CrashLoopBackOff"), &json!(3)),
        "what the node looked like right when the alert opened"
    );

    // the node moves on (more restarts, a new reason); the snapshot is not a running record of it
    store.set_status("w", "crit", "OOMKilled");
    store.set_meta("w", serde_json::from_value(json!({"restarts": 9})).unwrap());
    e.evaluate(3_000);
    let updated = db.alerts.get_alert(opened.id).unwrap().unwrap();
    assert_eq!(updated.title, "w OOMKilled", "the alert itself does update");
    assert_eq!(
        updated.snapshot, opened.snapshot,
        "but its snapshot stays what it was at the start"
    );
}

#[test]
fn heartbeats_are_recorded_only_when_the_status_changes() {
    let (e, store, db) = engine();
    store.set_topology("s", world(), vec![]);
    e.evaluate(1_000);
    e.evaluate(2_000);
    store.set_status("w", "warn", "");
    e.evaluate(3_000);
    // three buckets over 1000..4000: up from the first beat, still up (no new row was written), warn from 3000
    let up = db.beats.uptime(4_000, 3_000, 3).unwrap();
    assert_eq!(up["w"].bars, ["up", "up", "warn"]);
    assert_eq!(
        db.beats.uptime(4_000, 4_000, 4).unwrap()["w"].bars[0],
        "nodata",
        "before the first beat nothing is known"
    );
    assert_eq!(up["w"].current, "warn");
}

#[test]
fn active_alerts_survive_a_restart_and_are_not_opened_twice() {
    let (e, store, db) = engine();
    store.set_topology("s", world(), vec![]);
    store.set_status("w", "crit", "OOMKilled");
    e.evaluate(1_000);
    let again = EngineImp::new(
        store.clone(),
        db.alerts.clone(),
        db.sources.clone(),
        db.beats.clone(),
        db.settings.clone(),
    );
    again.evaluate(2_000);
    assert_eq!(db.alerts.list_alerts(10).unwrap().len(), 1);
}

#[test]
fn acknowledging_an_alert_is_stored_and_unknown_ones_are_refused() {
    let (e, store, db) = engine();
    store.set_topology("s", world(), vec![]);
    store.set_status("w", "crit", "x");
    e.evaluate(1_000);
    let id = db.alerts.active_alerts().unwrap()[0].id;
    assert!(e.ack(id, "admin"));
    let acked = db.alerts.get_alert(id).unwrap().unwrap();
    assert!(
        acked.ack && acked.ack_by == "admin" && acked.ack_ts.is_some(),
        "who and when are kept"
    );
    assert!(!e.ack(999, "admin"));
}

#[test]
fn the_settings_change_the_thresholds() {
    let (e, store, db) = engine();
    let mut nodes = world();
    nodes.push(Node {
        m: HashMap::from([("used".to_string(), 50.0)]),
        ..node("v", "volume", Some("h"), "ok", json!({"size": 100}))
    });
    store.set_topology("s", nodes, vec![]);
    e.evaluate(1_000);
    assert!(db.alerts.active_alerts().unwrap().is_empty());
    e.set_settings(br#"{"rules":[{"id":"volume-usage","enabled":true,"value":40,"crit":60}]}"#);
    e.evaluate(2_000);
    assert_eq!(db.alerts.active_alerts().unwrap()[0].sev, Severity::Warn);
    e.set_settings(b"not json"); // ignored: the rules stay
    e.evaluate(3_000);
    assert_eq!(db.alerts.active_alerts().unwrap().len(), 1);
}

#[test]
fn sizes_read_like_the_ones_of_the_go_hub() {
    assert_eq!(
        (
            fmt_size(0.5).as_str(),
            fmt_size(9.96).as_str(),
            fmt_size(120.0).as_str(),
            fmt_size(1536.0).as_str()
        ),
        ("0.5 GiB", "10.0 GiB", "120 GiB", "1.50 TiB")
    );
}
