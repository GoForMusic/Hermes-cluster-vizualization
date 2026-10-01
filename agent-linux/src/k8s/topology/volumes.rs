//! Persistent volume claims into volumes, placed on the host of the pod that mounts them (`workloads::build` says which). A claim
//! nobody mounts is not drawn: there is nowhere to place it. Pure, like the rest of the topology.

use std::collections::HashMap;

use hermes_proto::v1::{Node as PbNode, NodeKind, Own, Provider};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use super::super::kubelet::GIB;
use super::{meta, quantity_of};

/// The size a claim has: what the cluster granted, or else what was asked for.
pub fn claim_size(pvc: &PersistentVolumeClaim) -> f64 {
    let granted = quantity_of(
        pvc.status.as_ref().and_then(|s| s.capacity.as_ref()),
        "storage",
    )
    .unwrap_or(0.0);
    if granted > 0.0 {
        return granted;
    }
    quantity_of(
        pvc.spec
            .as_ref()
            .and_then(|s| s.resources.as_ref())
            .and_then(|r| r.requests.as_ref()),
        "storage",
    )
    .unwrap_or(0.0)
}

pub fn build(
    cid: &str,
    now_ms: i64,
    pvcs: &[PersistentVolumeClaim],
    pvc_node: &HashMap<String, String>,
    pvc_pods: &HashMap<String, Vec<String>>,
    volume_used: &HashMap<String, f64>,
) -> Vec<PbNode> {
    let host_id = |name: &str| format!("{cid}:n:{name}");
    let mut sorted: Vec<&PersistentVolumeClaim> = pvcs.iter().collect();
    sorted.sort_by_key(|p| {
        format!(
            "{}{}",
            p.metadata.namespace.as_deref().unwrap_or_default(),
            p.metadata.name.as_deref().unwrap_or_default()
        )
    });

    let mut out = Vec::new();
    for pvc in sorted {
        let ns = pvc.metadata.namespace.as_deref().unwrap_or_default();
        let name = pvc.metadata.name.as_deref().unwrap_or_default();
        let k = format!("{ns}/{name}");
        let Some(node) = pvc_node.get(&k) else {
            continue;
        }; // not mounted by a running pod: nothing to place it on
        let used = volume_used.get(&k);
        out.push(PbNode {
            id: format!("{cid}:v:{ns}:{name}"),
            kind: NodeKind::Volume.into(),
            name: name.to_string(),
            parent: Some(host_id(node)),
            provider: Provider::Kubernetes.into(),
            own: Own::Ok.into(),
            since: now_ms,
            m: HashMap::from([("used".to_string(), used.copied().unwrap_or(0.0))]),
            meta: meta(serde_json::json!({
                "size": claim_size(pvc) / GIB,
                "sc": pvc.spec.as_ref().and_then(|s| s.storage_class_name.clone()).unwrap_or_default(),
                "mountedBy": pvc_pods.get(&k).map(|p| p.join(", ")).unwrap_or_default(),
                "ns": ns,
                "usageKnown": used.is_some(),
            })),
            ..Default::default()
        });
    }
    out
}
