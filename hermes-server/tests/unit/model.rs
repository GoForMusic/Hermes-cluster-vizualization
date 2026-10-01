use hermes_proto::v1 as pb;
use serde_json::json;

use super::*;

#[test]
fn a_node_serialises_the_way_the_browser_expects() {
    let n = Node {
        id: "a".into(),
        kind: "host".into(),
        name: "a".into(),
        own: "ok".into(),
        status: "ok".into(),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&n).unwrap(),
        json!({"id":"a","kind":"host","name":"a","parent":null,"provider":"","own":"ok","status":"ok","reason":"","since":0,"m":{},"meta":{}})
    );
    let stale = Node { stale: true, ..n };
    assert_eq!(serde_json::to_value(&stale).unwrap()["stale"], json!(true));
}

#[test]
fn every_event_of_the_stream_has_a_shape_the_web_app_knows() {
    for event in [
        json!({"type": "snapshot", "nodes": [], "edges": []}),
        json!({"type": "status", "id": "n", "own": "crit", "reason": "OOM"}),
        json!({"type": "meta", "id": "n", "meta": {"restarts": 3}}),
        json!({"type": "metrics", "nodes": {"n": {"cpu": 1.0}}, "edges": {"e": 2.0}}),
        json!({"type": "alert", "alert": {"id": 1, "key": "k", "sev": "crit", "nodeId": "n", "title": "t", "detail": "d", "ts": 1, "resolvedTs": null, "ack": false}, "isNew": true}),
        json!({"type": "sources"}),
        json!({"type": "settings", "settings": {"title": "x"}}),
    ] {
        let parsed: HubEvent =
            serde_json::from_value(event.clone()).unwrap_or_else(|e| panic!("{event}: {e}"));
        assert_eq!(
            serde_json::to_value(&parsed).unwrap()["type"],
            event["type"]
        );
    }
}

#[test]
fn the_secret_of_a_source_never_reaches_the_browser() {
    let s = Source {
        id: "s1".into(),
        kind: TYPE_SWARM_AGENT.into(),
        secret: "hunter2".into(),
        ..Default::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(
        !json.contains("hunter2") && !json.contains("secret"),
        "{json}"
    );
    assert!(json.contains(r#""type":"Docker Swarm (agent)""#));
    assert!(!json.contains("builtin"), "omitted while false");
}

#[test]
fn an_alert_uses_camel_case() {
    let a = Alert {
        node_id: "n".into(),
        resolved_ts: Some(5),
        ..Default::default()
    };
    let v = serde_json::to_value(&a).unwrap();
    assert_eq!((&v["nodeId"], &v["resolvedTs"]), (&json!("n"), &json!(5)));
}

#[test]
fn the_provider_follows_the_source_type() {
    let of = |t: &str| {
        Source {
            kind: t.into(),
            ..Default::default()
        }
        .provider()
    };
    assert_eq!(
        (of("Docker Swarm (agent)"), of("Kubernetes"), of("Nomad")),
        ("swarm", "kubernetes", "nomad")
    );
}

#[test]
fn protobuf_nodes_become_browser_nodes() {
    let n = pb::Node {
        id: "n".into(),
        kind: pb::NodeKind::Workload.into(),
        provider: pb::Provider::Swarm.into(),
        own: pb::Own::Crit.into(),
        parent: Some("h".into()),
        meta: Some(hermes_proto::value::struct_from_json(
            json!({"restarts": 3}),
        )),
        ..Default::default()
    };
    let node = Node::from(n);
    assert_eq!(
        (
            node.kind.as_str(),
            node.provider.as_str(),
            node.own.as_str(),
            node.status.as_str()
        ),
        ("workload", "swarm", "crit", "crit")
    );
    assert_eq!(node.parent.as_deref(), Some("h"));
    assert_eq!(serde_json::to_string(&node.meta["restarts"]).unwrap(), "3");
}

#[test]
fn the_flows_event_the_hub_sends_is_the_one_the_web_app_reads() {
    let line = FlowLine {
        src: "s1:p:a".into(),
        dst: "s1:svc:b".into(),
        via: "s1:p:c".into(),
        port: 80,
        mbps: 1.5,
        external: false,
    };
    let sent = serde_json::json!({"type": "flows", "source": "s1", "flows": [line.clone()]});
    match serde_json::from_value::<HubEvent>(sent).unwrap() {
        HubEvent::Flows { source, flows } => {
            assert_eq!((source.as_str(), flows), ("s1", vec![line]))
        }
        other => panic!("{other:?}"),
    }
}
