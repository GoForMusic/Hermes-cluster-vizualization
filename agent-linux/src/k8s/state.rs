//! What the Kubernetes objects say about their own health, in the words of the hub: ok, warn or crit and a reason.

use hermes_proto::v1::Own;
use k8s_openapi::api::core::v1::{ContainerStatus, Node, Pod};

/// A node is critical when it is not ready, and a warning when it is under pressure.
pub fn node_state(n: &Node) -> (Own, String) {
    let conditions = n
        .status
        .as_ref()
        .and_then(|s| s.conditions.as_ref())
        .map(Vec::as_slice)
        .unwrap_or_default();
    if conditions
        .iter()
        .any(|c| c.type_ == "Ready" && c.status != "True")
    {
        return (Own::Crit, "NotReady".into());
    }
    for c in conditions {
        if matches!(
            c.type_.as_str(),
            "MemoryPressure" | "DiskPressure" | "PIDPressure"
        ) && c.status == "True"
        {
            return (Own::Warn, c.type_.clone());
        }
    }
    (Own::Ok, String::new())
}

const CRASH_REASONS: [&str; 7] = [
    "CrashLoopBackOff",
    "ImagePullBackOff",
    "ErrImagePull",
    "CreateContainerError",
    "CreateContainerConfigError",
    "InvalidImageName",
    "RunContainerError",
];

pub fn pod_state(p: &Pod) -> (Own, String) {
    let status = p.status.as_ref();
    let phase = status.and_then(|s| s.phase.as_deref()).unwrap_or_default();
    if phase == "Failed" {
        let reason = status
            .and_then(|s| s.reason.as_deref())
            .filter(|r| !r.is_empty())
            .unwrap_or("Failed");
        return (Own::Crit, reason.to_string());
    }
    let init = status
        .and_then(|s| s.init_container_statuses.as_deref())
        .unwrap_or_default();
    let regular = status
        .and_then(|s| s.container_statuses.as_deref())
        .unwrap_or_default();
    let never = p.spec.as_ref().and_then(|s| s.restart_policy.as_deref()) == Some("Never");
    for cs in init.iter().chain(regular) {
        if let Some(reason) = cs
            .state
            .as_ref()
            .and_then(|s| s.waiting.as_ref())
            .and_then(|w| w.reason.as_deref())
            .filter(|r| CRASH_REASONS.contains(r))
        {
            return (Own::Crit, reason.to_string());
        }
        if let Some(t) = cs
            .state
            .as_ref()
            .and_then(|s| s.terminated.as_ref())
            .filter(|t| t.exit_code != 0 && !never)
        {
            return (
                Own::Crit,
                t.reason
                    .clone()
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "Error".into()),
            );
        }
    }
    if p.metadata.deletion_timestamp.is_some() {
        return (Own::Warn, "Terminating".into());
    }
    if phase == "Pending" {
        return (Own::Warn, "Pending".into());
    }
    if regular.iter().any(|cs| !cs.ready) {
        return (Own::Warn, "NotReady".into());
    }
    (Own::Ok, String::new())
}

pub fn container_state(cs: &ContainerStatus) -> String {
    let Some(state) = cs.state.as_ref() else {
        return "unknown".into();
    };
    if state.running.is_some() {
        "running".into()
    } else if let Some(w) = &state.waiting {
        format!("waiting: {}", w.reason.as_deref().unwrap_or_default())
    } else if let Some(t) = &state.terminated {
        format!("terminated: {}", t.reason.as_deref().unwrap_or_default())
    } else {
        "unknown".into()
    }
}

/// What controls the pod and a display name: a Deployment's pods get `name-xxxxx`, the hash of the ReplicaSet left out.
pub fn pod_kind(p: &Pod) -> (String, String) {
    let name = p.metadata.name.clone().unwrap_or_default();
    let Some(owner) = p.metadata.owner_references.as_ref().and_then(|o| o.first()) else {
        return ("Pod".into(), name);
    };
    if owner.kind == "ReplicaSet" {
        let deployment = owner
            .name
            .rsplit_once('-')
            .filter(|(head, _)| !head.is_empty())
            .map_or(owner.name.as_str(), |(head, _)| head);
        let suffix = name
            .rsplit_once('-')
            .map_or(name.as_str(), |(_, tail)| tail);
        return ("Deployment".into(), format!("{deployment}-{suffix}"));
    }
    (owner.kind.clone(), name)
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_state.rs"]
mod tests;
