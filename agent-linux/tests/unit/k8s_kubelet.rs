use super::*;

const GI: f64 = GIB;

#[test]
fn a_volume_on_its_own_filesystem_is_measured_and_a_directory_on_a_shared_disk_is_not() {
    for (what, capacity, requested, want) in [
        (
            "block volume, a little smaller than requested (ext4 overhead)",
            1.96 * GI,
            2.0 * GI,
            true,
        ),
        ("exactly the requested size", 2.0 * GI, 2.0 * GI, true),
        (
            "local-path directory: the whole node disk is reported",
            3.02 * GI,
            2.0 * GI,
            false,
        ),
        ("much smaller than requested", 0.5 * GI, 2.0 * GI, false),
        ("the kubelet reported no capacity", 0.0, 2.0 * GI, false),
        ("the size of the PVC is unknown", 2.0 * GI, 0.0, false),
    ] {
        assert_eq!(own_filesystem(capacity, requested), want, "{what}");
    }
}

fn summary(second: (u64, u64), time: &str) -> Summary {
    parse_summary(&format!(
        r#"{{"node": {{}}, "pods": [
            {{"podRef": {{"name": "web-1", "namespace": "default"}}, "network": {{"time": "{time}", "rxBytes": {}, "txBytes": {}}},
              "volume": [{{"usedBytes": 1073741824, "capacityBytes": 2000000000, "pvcRef": {{"name": "data", "namespace": "default"}}}}, {{"name": "config"}}]}},
            {{"podRef": {{"name": "host-1", "namespace": "kube-system"}}}}
        ]}}"#,
        second.0, second.1
    ))
    .unwrap()
}

#[test]
fn volume_usage_and_pod_rates_come_out_of_two_summaries() {
    let (mut volumes, mut net, mut rates) = (HashMap::new(), Tracker::new(), HashMap::new());
    let pvc_size = HashMap::from([("default/data".to_string(), 2e9)]);
    let mut seen = HashSet::new();
    absorb(
        &summary((1_000_000, 0), "2026-09-21T10:00:00Z"),
        &pvc_size,
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    assert!(
        rates.is_empty(),
        "the first reading has nothing to compare with"
    );
    assert!((volumes["default/data"] - 1.0).abs() < 1e-9, "1 GiB used");
    assert_eq!(
        seen,
        HashSet::from(["default/web-1".to_string()]),
        "a pod with no network of its own is not tracked"
    );

    absorb(
        &summary((2_250_000, 500_000), "2026-09-21T10:00:10Z"),
        &pvc_size,
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    let (rx, tx) = rates["default/web-1"];
    assert!(
        (rx - 1.0).abs() < 1e-9 && (tx - 0.4).abs() < 1e-9,
        "{rx} {tx}"
    );
}

#[test]
fn a_volume_that_shares_a_disk_is_left_unknown() {
    let (mut volumes, mut net, mut rates, mut seen) = (
        HashMap::from([("default/data".to_string(), 9.0)]),
        Tracker::new(),
        HashMap::new(),
        HashSet::new(),
    );
    let pvc_size = HashMap::from([("default/data".to_string(), 2.0 * GI)]); // asked for 2 GiB, the kubelet reports 2 GB: fine
    absorb(
        &summary((0, 0), "2026-09-21T10:00:00Z"),
        &pvc_size,
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    assert!(volumes.contains_key("default/data"));
    let pvc_size = HashMap::from([("default/data".to_string(), 100.0 * GI)]); // it reports far less than asked for: the disk is not its own
    absorb(
        &summary((0, 0), "2026-09-21T10:00:00Z"),
        &pvc_size,
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    assert!(
        !volumes.contains_key("default/data"),
        "an old figure must not outlive the reason to trust it"
    );
}

#[test]
fn only_the_interface_list_is_used_when_that_is_all_there_is() {
    let s = parse_summary(r#"{"pods": [{"podRef": {"name": "p", "namespace": "n"}, "network": {"time": "2026-09-21T10:00:00Z", "interfaces": [{"rxBytes": 5, "txBytes": 7}, {"rxBytes": 1, "txBytes": 1}]}}]}"#).unwrap();
    let (mut volumes, mut net, mut rates, mut seen) = (
        HashMap::new(),
        Tracker::new(),
        HashMap::new(),
        HashSet::new(),
    );
    absorb(
        &s,
        &HashMap::new(),
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    let s2 = parse_summary(r#"{"pods": [{"podRef": {"name": "p", "namespace": "n"}, "network": {"time": "2026-09-21T10:00:01Z", "interfaces": [{"rxBytes": 125005, "txBytes": 7}, {"rxBytes": 1, "txBytes": 1}]}}]}"#).unwrap();
    absorb(
        &s2,
        &HashMap::new(),
        &mut volumes,
        &mut net,
        &mut seen,
        &mut rates,
    );
    assert!(
        (rates["n/p"].0 - 1.0).abs() < 1e-9,
        "125000 bytes in a second is 1 Mb/s"
    );
}

#[test]
fn garbage_is_an_error_and_an_empty_summary_is_nothing() {
    assert!(parse_summary("not json").is_err());
    assert!(parse_summary("{}").unwrap().pods.is_empty());
}
