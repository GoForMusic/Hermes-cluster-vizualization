use serde_json::{Value, json};

use super::*;

fn pod(v: Value) -> Pod {
    serde_json::from_value(json!({"metadata": {"name": "web-7d9f6c5b4-x2k9p", "namespace": "default"}, "spec": {"containers": []}, "status": {}}).as_object().map(|base| {
        let mut m = base.clone();
        for (k, x) in v.as_object().unwrap() {
            m.insert(k.clone(), x.clone());
        }
        Value::Object(m)
    }).unwrap())
    .unwrap()
}

fn state(v: Value) -> (Own, String) {
    pod_state(&pod(v))
}

fn running(ready: bool) -> Value {
    json!({"status": {"phase": "Running", "containerStatuses": [{"name": "c", "image": "i", "imageID": "", "ready": ready, "restartCount": 0, "state": {"running": {}}}]}})
}

#[test]
fn a_running_ready_pod_is_ok_and_one_that_is_not_ready_is_a_warning() {
    assert_eq!(state(running(true)), (Own::Ok, String::new()));
    assert_eq!(state(running(false)), (Own::Warn, "NotReady".into()));
}

#[test]
fn a_pod_that_cannot_start_is_critical_with_the_reason_kubernetes_gives() {
    for reason in [
        "CrashLoopBackOff",
        "ImagePullBackOff",
        "ErrImagePull",
        "CreateContainerConfigError",
    ] {
        let v = json!({"status": {"phase": "Running", "containerStatuses": [{"name": "c", "image": "i", "imageID": "", "ready": false, "restartCount": 5, "state": {"waiting": {"reason": reason}}}]}});
        assert_eq!(state(v), (Own::Crit, reason.into()));
    }
    let waiting = json!({"status": {"phase": "Pending", "containerStatuses": [{"name": "c", "image": "i", "imageID": "", "ready": false, "restartCount": 0, "state": {"waiting": {"reason": "ContainerCreating"}}}]}});
    assert_eq!(
        state(waiting),
        (Own::Warn, "Pending".into()),
        "still starting is not a failure"
    );
}

#[test]
fn a_failed_init_container_counts_too() {
    let v = json!({"status": {"phase": "Pending", "initContainerStatuses": [{"name": "i", "image": "i", "imageID": "", "ready": false, "restartCount": 3, "state": {"waiting": {"reason": "CrashLoopBackOff"}}}]}});
    assert_eq!(state(v), (Own::Crit, "CrashLoopBackOff".into()));
}

#[test]
fn a_container_that_exited_with_an_error_is_critical_unless_it_is_never_restarted() {
    let status = |code| json!({"phase": "Running", "containerStatuses": [{"name": "c", "image": "i", "imageID": "", "ready": false, "restartCount": 1, "state": {"terminated": {"exitCode": code, "reason": "OOMKilled"}}}]});
    assert_eq!(
        state(json!({"status": status(137)})),
        (Own::Crit, "OOMKilled".into())
    );
    assert_eq!(
        state(json!({"status": status(0)})).0,
        Own::Warn,
        "a clean exit is not a failure"
    );
    let never = json!({"spec": {"containers": [], "restartPolicy": "Never"}, "status": status(1)});
    assert_eq!(
        state(never).0,
        Own::Warn,
        "a job that ran and failed is not what this watches"
    );
}

#[test]
fn a_pod_that_failed_says_why() {
    assert_eq!(
        state(json!({"status": {"phase": "Failed", "reason": "Evicted"}})),
        (Own::Crit, "Evicted".into())
    );
    assert_eq!(
        state(json!({"status": {"phase": "Failed"}})),
        (Own::Crit, "Failed".into())
    );
}

#[test]
fn a_pod_being_deleted_or_waiting_to_be_scheduled_is_a_warning() {
    let terminating = json!({"metadata": {"name": "p", "deletionTimestamp": "2026-09-21T10:00:00Z"}, "status": {"phase": "Running"}});
    assert_eq!(state(terminating), (Own::Warn, "Terminating".into()));
    assert_eq!(
        state(json!({"status": {"phase": "Pending"}})),
        (Own::Warn, "Pending".into())
    );
}

fn node(conditions: Value) -> Node {
    serde_json::from_value(json!({"metadata": {"name": "n"}, "status": {"conditions": conditions}}))
        .unwrap()
}

#[test]
fn a_node_that_is_not_ready_is_critical_and_one_under_pressure_is_a_warning() {
    let cond = |t: &str, s: &str| json!({"type": t, "status": s});
    assert_eq!(
        node_state(&node(json!([
            cond("Ready", "True"),
            cond("MemoryPressure", "False")
        ]))),
        (Own::Ok, String::new())
    );
    assert_eq!(
        node_state(&node(json!([cond("Ready", "False")]))),
        (Own::Crit, "NotReady".into())
    );
    assert_eq!(
        node_state(&node(json!([cond("Ready", "Unknown")]))).0,
        Own::Crit,
        "a node that stopped reporting is not ready"
    );
    assert_eq!(
        node_state(&node(json!([
            cond("Ready", "True"),
            cond("DiskPressure", "True")
        ]))),
        (Own::Warn, "DiskPressure".into())
    );
    assert_eq!(
        node_state(&node(json!([
            cond("Ready", "False"),
            cond("DiskPressure", "True")
        ])))
        .1,
        "NotReady",
        "not ready outranks pressure"
    );
    assert_eq!(node_state(&node(json!([]))).0, Own::Ok);
}

#[test]
fn the_name_of_a_pod_says_what_controls_it() {
    let owned = |kind: &str, name: &str| {
        pod(
            json!({"metadata": {"name": "web-7d9f6c5b4-x2k9p", "ownerReferences": [{"apiVersion": "v1", "kind": kind, "name": name, "uid": "u"}]}}),
        )
    };
    assert_eq!(
        pod_kind(&owned("ReplicaSet", "web-7d9f6c5b4")),
        ("Deployment".into(), "web-x2k9p".into())
    );
    assert_eq!(
        pod_kind(&owned("StatefulSet", "db")),
        ("StatefulSet".into(), "web-7d9f6c5b4-x2k9p".into())
    );
    assert_eq!(
        pod_kind(&pod(json!({"metadata": {"name": "solo"}}))),
        ("Pod".into(), "solo".into())
    );
}

#[test]
fn a_container_says_what_it_is_doing() {
    let cs = |state: Value| -> ContainerStatus {
        serde_json::from_value(json!({"name": "c", "image": "i", "imageID": "", "ready": false, "restartCount": 0, "state": state})).unwrap()
    };
    assert_eq!(container_state(&cs(json!({"running": {}}))), "running");
    assert_eq!(
        container_state(&cs(json!({"waiting": {"reason": "ErrImagePull"}}))),
        "waiting: ErrImagePull"
    );
    assert_eq!(
        container_state(&cs(
            json!({"terminated": {"exitCode": 1, "reason": "Error"}})
        )),
        "terminated: Error"
    );
}
