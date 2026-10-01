//! Kubernetes objects into what the hub draws: a cluster, its nodes as hosts, its pods as workloads, the claims that pods mount as volumes,
//! and the control-plane links. Pure: everything it needs is passed in, so it is tested with objects as the API sends them.
//!
//! Orchestration only: nodes -> hosts, pods -> workloads and claims -> volumes each have their own file (`hosts`, `workloads`, `volumes`),
//! since each is its own self-contained pass over one kind of object; this file wires their outputs together in the order they depend on
//! each other (a workload needs its host's allocatable resources, a volume needs which host mounts it).

use std::collections::{BTreeMap, HashMap, HashSet};

use hermes_proto::v1::{Edge, EdgeType, Node as PbNode, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use k8s_openapi::api::core::v1::{Node, PersistentVolumeClaim, Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::{Ingress, NetworkPolicy};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::DynamicObject;
use serde_json::Value;

use super::metrics::Usage;

mod hosts;
mod volumes;
mod workloads;

pub use volumes::claim_size;

pub struct Inputs<'a> {
    pub source_id: &'a str,
    pub source_name: &'a str,
    pub version: &'a str,
    pub api: &'a str,
    /// The UID of the `kube-system` namespace: identifies the cluster (empty when it could not be read).
    pub uid: &'a str,
    pub now_ms: i64,
    pub nodes: &'a [Node],
    pub pods: &'a [Pod],
    pub pvcs: &'a [PersistentVolumeClaim],
    pub services: &'a [Service],
    pub slices: &'a [EndpointSlice],
    pub ingresses: &'a [Ingress],
    pub gateways: &'a [DynamicObject],
    pub routes: &'a [DynamicObject],
    pub policies: &'a [NetworkPolicy],
    pub node_usage: &'a HashMap<String, Usage>,
    pub pod_usage: &'a HashMap<String, Usage>,
    /// `ns/pvc` -> GiB used, for the volumes the kubelet could measure.
    pub volume_used: &'a HashMap<String, f64>,
    /// `ns/pod` -> (received, sent) in Mb/s.
    pub pod_rates: &'a HashMap<String, (f64, f64)>,
}

/// Shared by `hosts`/`volumes`: reads a resource quantity (CPU, memory, storage) out of the map the API gives it, in whatever unit
/// Kubernetes wrote it (`quantity::parse` normalizes that).
fn quantity_of(map: Option<&BTreeMap<String, Quantity>>, key: &str) -> Option<f64> {
    map?.get(key).and_then(|q| super::quantity::parse(&q.0))
}

/// Shared by `hosts`/`workloads`/`volumes`: every node's `meta` is a `prost_types::Struct`, built from a `serde_json::json!` object.
fn meta(v: Value) -> Option<prost_types::Struct> {
    Some(struct_from_json(v))
}

pub fn build(i: &Inputs<'_>) -> (Vec<PbNode>, Vec<Edge>) {
    let cid = i.source_id;
    let mut out = vec![PbNode {
        id: cid.to_string(),
        kind: NodeKind::Cluster.into(),
        name: i.source_name.to_string(),
        provider: Provider::Kubernetes.into(),
        own: Own::Ok.into(),
        since: i.now_ms,
        meta: meta(serde_json::json!({"version": i.version, "api": i.api, "uid": i.uid})),
        ..Default::default()
    }];

    let hosts = hosts::build(cid, i.now_ms, i.nodes, i.node_usage);
    out.extend(hosts.nodes);

    let wl = workloads::build(
        cid,
        i.now_ms,
        i.pods,
        &hosts.alloc_cpu,
        &hosts.alloc_mem,
        i.pod_usage,
        i.pod_rates,
    );
    out.extend(wl.nodes);

    out.extend(volumes::build(
        cid,
        i.now_ms,
        i.pvcs,
        &wl.pvc_node,
        &wl.pvc_pods,
        i.volume_used,
    ));

    // ---- services, ingresses and "outside": how traffic gets to the pods
    let pod_ids: HashSet<String> = out
        .iter()
        .filter(|n| n.kind() == NodeKind::Workload)
        .map(|n| n.id.clone())
        .collect();
    let (networks, mut edges) = super::network::build(&super::network::Inputs {
        source_id: cid,
        now_ms: i.now_ms,
        services: i.services,
        slices: i.slices,
        ingresses: i.ingresses,
        gateways: i.gateways,
        routes: i.routes,
        policies: i.policies,
        pods: i.pods,
        pod_ids: &pod_ids,
    });
    out.extend(networks);

    // ---- control-plane -> worker links (the API gives no traffic figures for them, so they carry no numbers)
    let host_id = |name: &str| format!("{cid}:n:{name}");
    edges.extend(
        hosts
            .control
            .iter()
            .flat_map(|c| hosts.workers.iter().map(move |w| (c, w)))
            .map(|(c, w)| Edge {
                id: format!("{}>{}", host_id(c), host_id(w)),
                from: host_id(c),
                to: host_id(w),
                r#type: EdgeType::Control.into(),
                ..Default::default()
            }),
    );
    (out, edges)
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_topology.rs"]
mod tests;
