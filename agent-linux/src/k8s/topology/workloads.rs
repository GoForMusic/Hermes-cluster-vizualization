//! Pods into workloads: their status, containers, resource usage and network rates. Also collects, for `volumes::build`
//! afterwards, which host and which pods mount each claim. Pure, like the rest of the topology.

use std::collections::HashMap;

use hermes_proto::v1::{Node as PbNode, NodeKind, Provider};
use k8s_openapi::api::core::v1::Pod;
use serde_json::{Value, json};

use super::super::metrics::Usage;
use super::super::network::is_ingress_controller;
use super::super::state::{container_state, pod_kind, pod_state};
use super::meta;

pub struct Workloads {
    pub nodes: Vec<PbNode>,
    /// `ns/pvc` -> the host of the pod that mounts it.
    pub pvc_node: HashMap<String, String>,
    /// `ns/pvc` -> the names of the pods that mount it (for the volume's `mountedBy`).
    pub pvc_pods: HashMap<String, Vec<String>>,
}

/// What a pod says about itself, plus `ingressController` when it is one: the hub then knows which traffic is the entry to the cluster.
fn pod_meta(p: &Pod, mut m: Value) -> Value {
    if is_ingress_controller(p) {
        m["ingressController"] = json!(true);
    }
    m
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    cid: &str,
    now_ms: i64,
    pods: &[Pod],
    alloc_cpu: &HashMap<String, f64>,
    alloc_mem: &HashMap<String, f64>,
    pod_usage: &HashMap<String, Usage>,
    pod_rates: &HashMap<String, (f64, f64)>,
) -> Workloads {
    let host_id = |name: &str| format!("{cid}:n:{name}");
    let key = |p: &Pod| {
        format!(
            "{}{}",
            p.metadata.namespace.as_deref().unwrap_or_default(),
            p.metadata.name.as_deref().unwrap_or_default()
        )
    };
    let mut sorted: Vec<&Pod> = pods.iter().collect();
    sorted.sort_by_key(|p| key(p));

    let mut out = Workloads {
        nodes: Vec::new(),
        pvc_node: HashMap::new(),
        pvc_pods: HashMap::new(),
    };
    for p in sorted {
        let node = p
            .spec
            .as_ref()
            .and_then(|s| s.node_name.as_deref())
            .unwrap_or_default();
        let phase = p
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or_default();
        if node.is_empty() || phase == "Succeeded" || !alloc_cpu.contains_key(node) {
            continue;
        }
        let ns = p.metadata.namespace.as_deref().unwrap_or_default();
        let pod_name = p.metadata.name.as_deref().unwrap_or_default();
        let (own, reason) = pod_state(p);
        let (typ, name) = pod_kind(p);

        let statuses: HashMap<&str, &k8s_openapi::api::core::v1::ContainerStatus> = p
            .status
            .as_ref()
            .and_then(|s| s.container_statuses.as_ref())
            .into_iter()
            .flatten()
            .map(|c| (c.name.as_str(), c))
            .collect();
        let restarts: i64 = statuses.values().map(|c| i64::from(c.restart_count)).sum();
        let containers: Vec<Value> = p
            .spec
            .as_ref()
            .map(|s| s.containers.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|c| {
                let cs = statuses.get(c.name.as_str());
                json!({
                    "name": c.name, "image": c.image.clone().unwrap_or_default(), "ready": cs.is_some_and(|s| s.ready),
                    "restarts": cs.map_or(0, |s| s.restart_count), "state": cs.map_or_else(|| "unknown".to_string(), |s| container_state(s)),
                })
            })
            .collect();
        let image = p
            .spec
            .as_ref()
            .and_then(|s| s.containers.first())
            .and_then(|c| c.image.clone())
            .unwrap_or_default();

        let mut m = HashMap::new();
        let usage_key = format!("{ns}/{pod_name}");
        if let Some(u) = pod_usage.get(&usage_key) {
            if let Some(a) = alloc_cpu.get(node).filter(|a| **a > 0.0) {
                m.insert("cpu".to_string(), u.milli / a * 100.0);
            }
            if let Some(a) = alloc_mem.get(node).filter(|a| **a > 0.0) {
                m.insert("mem".to_string(), u.bytes / a * 100.0);
            }
            m.insert("cpuMilli".to_string(), u.milli);
            m.insert("memMiB".to_string(), u.bytes / (1 << 20) as f64);
        }
        // Measured by the kubelet, so it is the same on any CNI or distribution. A pod on the host network shares the node's interface and the
        // kubelet reports the whole node's traffic for it: not the pod's own, so it is left out.
        let host_network = p
            .spec
            .as_ref()
            .and_then(|s| s.host_network)
            .unwrap_or(false);
        if let Some((rx, tx)) = pod_rates.get(&usage_key).filter(|_| !host_network) {
            m.insert("rxMbps".to_string(), *rx);
            m.insert("txMbps".to_string(), *tx);
        }
        out.nodes.push(PbNode {
            id: format!("{cid}:p:{ns}:{pod_name}"),
            kind: NodeKind::Workload.into(),
            name: name.clone(),
            parent: Some(host_id(node)),
            provider: Provider::Kubernetes.into(),
            own: own.into(),
            reason,
            since: now_ms,
            m,
            meta: meta(pod_meta(p, json!({"type": typ, "image": image, "ns": ns, "restarts": restarts, "containers": containers, "phase": phase, "podName": pod_name, "podIP": p.status.as_ref().and_then(|s| s.pod_ip.clone()).unwrap_or_default()}))),
        });

        for v in p
            .spec
            .as_ref()
            .and_then(|s| s.volumes.as_ref())
            .into_iter()
            .flatten()
        {
            if let Some(claim) = &v.persistent_volume_claim {
                let k = format!("{ns}/{}", claim.claim_name);
                out.pvc_node.insert(k.clone(), node.to_string());
                out.pvc_pods.entry(k).or_default().push(name.clone());
            }
        }
    }
    out
}
