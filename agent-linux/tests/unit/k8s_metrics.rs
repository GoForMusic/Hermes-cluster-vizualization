use super::*;

#[test]
fn nodes_are_read_by_name() {
    let m = parse_nodes(
        r#"{"items": [{"metadata": {"name": "w1"}, "usage": {"cpu": "250m", "memory": "1Gi"}}]}"#,
    );
    assert_eq!(
        m["w1"],
        Usage {
            milli: 250.0,
            bytes: 1_073_741_824.0
        }
    );
}

#[test]
fn a_pod_adds_up_its_containers() {
    let m = parse_pods(
        r#"{"items": [{"metadata": {"name": "web", "namespace": "default"}, "containers": [{"usage": {"cpu": "100m", "memory": "64Mi"}}, {"usage": {"cpu": "50m", "memory": "32Mi"}}]}]}"#,
    );
    assert_eq!(
        m["default/web"],
        Usage {
            milli: 150.0,
            bytes: 96.0 * 1024.0 * 1024.0
        }
    );
}

#[test]
fn a_cluster_without_metrics_has_none() {
    assert!(
        parse_nodes("").is_empty()
            && parse_pods("{\"kind\": \"Status\"}").is_empty()
            && parse_nodes("not json").is_empty()
    );
}
