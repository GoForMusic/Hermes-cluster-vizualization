use hermes_proto::value::json_from_struct;
use serde_json::{Value, json};

use super::*;

fn volume(v: Value) -> VolumeInfo {
    serde_json::from_value(v).unwrap()
}

fn container(service: &str, mounts: Value) -> LocalContainer {
    serde_json::from_value(
        json!({"Id": "c", "Labels": {"com.docker.swarm.service.name": service}, "Mounts": mounts}),
    )
    .unwrap()
}

#[test]
fn a_named_volume_becomes_a_volume_node_on_this_host_with_what_it_takes_and_who_uses_it() {
    let volumes = [volume(
        json!({"Name": "demo_data", "Driver": "local", "Labels": {"com.docker.stack.namespace": "demo"}, "UsageData": {"Size": 1610612736u64, "RefCount": 1}}),
    )];
    let mounted = mounted_by(&[container(
        "demo_writer",
        json!([{"Type": "volume", "Name": "demo_data"}]),
    )]);
    let (nodes, used) = build("s1", "w1", &volumes, &mounted, 42);
    assert_eq!(nodes.len(), 1);
    let n = &nodes[0];
    assert_eq!(
        (
            n.id.as_str(),
            n.name.as_str(),
            n.parent.as_deref(),
            n.kind(),
            n.since
        ),
        (
            "s1:v:w1:demo_data",
            "demo_data",
            Some("s1:n:w1"),
            NodeKind::Volume,
            42
        )
    );
    assert_eq!(
        json_from_struct(n.meta.as_ref().unwrap()),
        json!({"ns": "demo", "sc": "local", "mountedBy": "demo_writer", "usageKnown": true}),
        "there is no size: a local volume has no limit"
    );
    assert_eq!((n.m["used"], used["s1:v:w1:demo_data"]), (1.5, 1.5));
}

#[test]
fn volumes_nobody_named_or_nobody_measured_are_left_out() {
    let anonymous = "a".repeat(64);
    let volumes = [
        volume(json!({"Name": anonymous, "UsageData": {"Size": 10}})),
        volume(json!({"Name": "not_measured", "UsageData": {"Size": -1}})),
        volume(json!({"Name": "no_usage_data"})),
        volume(json!({"Name": "kept", "UsageData": {"Size": 0}})),
    ];
    let (nodes, _) = build("s1", "w1", &volumes, &HashMap::new(), 0);
    assert_eq!(
        nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["kept"]
    );
    assert!(
        is_anonymous(&"0123456789abcdef".repeat(4))
            && !is_anonymous("demo_data")
            && !is_anonymous(&"g".repeat(64))
    );
}

#[test]
fn volumes_come_out_sorted_and_a_volume_used_by_two_services_names_both_once() {
    let volumes = [
        volume(json!({"Name": "b", "UsageData": {"Size": 1}})),
        volume(json!({"Name": "a", "UsageData": {"Size": 1}})),
    ];
    let mounted = mounted_by(&[
        container(
            "web",
            json!([{"Type": "volume", "Name": "a"}, {"Type": "bind", "Name": ""}]),
        ),
        container("job", json!([{"Type": "volume", "Name": "a"}])),
        container("web", json!([{"Type": "volume", "Name": "a"}])),
    ]);
    let (nodes, _) = build("s", "n", &volumes, &mounted, 0);
    assert_eq!(
        nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(
        json_from_struct(nodes[0].meta.as_ref().unwrap())["mountedBy"],
        "job, web"
    );
    assert_eq!(
        json_from_struct(nodes[1].meta.as_ref().unwrap())["mountedBy"],
        ""
    );
}
