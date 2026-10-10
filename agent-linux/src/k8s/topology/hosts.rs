//! Kubernetes nodes into hosts: their status, resources and CPU/memory load, plus what `topology::build` needs afterwards — how much
//! each host has to give out (for `workloads::build`'s percentages) and which side of the control plane it is on (for the
//! control-plane -> worker links). Pure, like the rest of the topology.

use std::collections::HashMap;

use hermes_proto::v1::{Node as PbNode, NodeKind, Provider};
use k8s_openapi::api::core::v1::Node;

use super::super::kubelet::GIB;
use super::super::metrics::Usage;
use super::super::state::node_state;
use super::{meta, quantity_of};

pub struct Hosts {
    pub nodes: Vec<PbNode>,
    /// host name -> allocatable millicores/bytes, for `workloads::build` to turn a pod's usage into a percentage of its host.
    pub alloc_cpu: HashMap<String, f64>,
    pub alloc_mem: HashMap<String, f64>,
    /// host names on each side of the control plane, for the control -> worker links `topology::build` draws afterwards.
    pub control: Vec<String>,
    pub workers: Vec<String>,
}

fn name_of(n: &Node) -> &str {
    n.metadata.name.as_deref().unwrap_or_default()
}

pub fn build(cid: &str, now_ms: i64, nodes: &[Node], node_usage: &HashMap<String, Usage>) -> Hosts {
    let host_id = |name: &str| format!("{cid}:n:{name}");
    let mut sorted: Vec<&Node> = nodes.iter().collect();
    sorted.sort_by(|a, b| name_of(a).cmp(name_of(b)));

    let mut out = Hosts {
        nodes: Vec::new(),
        alloc_cpu: HashMap::new(),
        alloc_mem: HashMap::new(),
        control: Vec::new(),
        workers: Vec::new(),
    };
    for n in sorted {
        let name = name_of(n);
        let labels = n.metadata.labels.as_ref();
        let control_plane = labels.is_some_and(|l| {
            l.contains_key("node-role.kubernetes.io/control-plane")
                || l.contains_key("node-role.kubernetes.io/master")
        });
        let role = if control_plane {
            "control-plane"
        } else {
            "worker"
        };
        let status = n.status.as_ref();
        let ip = status
            .and_then(|s| s.addresses.as_ref())
            .and_then(|a| a.iter().find(|a| a.type_ == "InternalIP"))
            .map(|a| a.address.clone())
            .unwrap_or_default();
        let (own, reason) = node_state(n);

        let allocatable = status.and_then(|s| s.allocatable.as_ref());
        let capacity = status.and_then(|s| s.capacity.as_ref());
        let ac = quantity_of(allocatable, "cpu").map_or(0.0, |c| (c * 1000.0).round());
        let am = quantity_of(allocatable, "memory").unwrap_or(0.0);
        out.alloc_cpu.insert(name.to_string(), ac);
        out.alloc_mem.insert(name.to_string(), am);
        let mut m = HashMap::new();
        if let Some(u) = node_usage.get(name).filter(|_| ac > 0.0 && am > 0.0) {
            m.insert("cpu".to_string(), u.milli / ac * 100.0);
            m.insert("mem".to_string(), u.bytes / am * 100.0);
            m.insert("cpuMilli".to_string(), u.milli);
            m.insert("memMiB".to_string(), u.bytes / (1 << 20) as f64);
        }
        let info = status.and_then(|s| s.node_info.as_ref());
        let mut host_meta = serde_json::json!({
            "ip": ip,
            "role": role,
            "vcpu": quantity_of(capacity, "cpu").unwrap_or(0.0).ceil(),
            "ram": quantity_of(capacity, "memory").unwrap_or(0.0) / GIB,
            "os": info.map(|x| x.os_image.as_str()).unwrap_or_default(),
            "kubelet": info.map(|x| x.kubelet_version.as_str()).unwrap_or_default(),
            "osType": info.map(|x| x.operating_system.as_str()).unwrap_or_default(), // linux | windows
            "arch": info.map(|x| x.architecture.as_str()).unwrap_or_default(),
        });
        if let Some(location) = hermes_agentkit::location::from_labels(|k| {
            labels.and_then(|l| l.get(k)).map(String::as_str)
        }) {
            host_meta["location"] = serde_json::json!(location);
        }
        out.nodes.push(PbNode {
            id: host_id(name),
            kind: NodeKind::Host.into(),
            name: name.to_string(),
            parent: Some(cid.to_string()),
            provider: Provider::Kubernetes.into(),
            own: own.into(),
            reason,
            since: now_ms,
            m,
            meta: meta(host_meta),
        });
        if control_plane {
            out.control.push(name.to_string())
        } else {
            out.workers.push(name.to_string())
        }
    }
    out
}
