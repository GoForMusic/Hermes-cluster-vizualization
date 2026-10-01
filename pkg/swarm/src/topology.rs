//! The swarm into what the hub draws: a cluster, its nodes as hosts, its tasks as workloads, and manager-to-worker control links. Pure:
//! everything it needs is passed in. `node_state`/`task_state`/`failed_recently` (raw Docker string interpretation) live in `state.rs`;
//! this file is the assembly.

use std::collections::HashMap;

use hermes_proto::v1::{Edge, EdgeType, Node as PbNode, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use serde_json::json;

use crate::docker::{EngineInfo, SwarmNetwork, SwarmNode, SwarmService, SwarmTask};
use crate::state::{TaskState, failed_recently, first_non_empty, node_state, task_state};

pub struct Inputs<'a> {
    pub source_id: &'a str,
    pub source_name: &'a str,
    pub info: &'a EngineInfo,
    pub nodes: &'a [SwarmNode],
    pub services: &'a [SwarmService],
    pub tasks: &'a [SwarmTask],
    pub networks: &'a [SwarmNetwork],
    pub now_ms: i64,
}

/// Docker's architecture names as they are used elsewhere (Kubernetes, image platforms).
pub fn normalize_arch(arch: &str) -> String {
    match arch.to_lowercase().as_str() {
        "x86_64" => "amd64".into(),
        "aarch64" => "arm64".into(),
        other => other.into(),
    }
}

/// `nginx:alpine@sha256:…` is `nginx:alpine`: Docker adds the digest to images in swarm mode.
pub fn short_image(image: &str) -> &str {
    match image.find('@') {
        Some(i) if i > 0 => &image[..i],
        _ => image,
    }
}

fn short(s: &str, n: usize) -> &str {
    s.get(..n).unwrap_or(s)
}

pub fn build(i: &Inputs<'_>) -> (Vec<PbNode>, Vec<Edge>) {
    let cid = i.source_id;
    let host_id = |node_id: &str| format!("{cid}:n:{node_id}");
    let mut out = vec![PbNode {
        id: cid.to_string(),
        kind: NodeKind::Cluster.into(),
        name: i.source_name.to_string(),
        provider: Provider::Swarm.into(),
        own: Own::Ok.into(),
        since: i.now_ms,
        meta: Some(struct_from_json(json!({
            "version": format!("Docker {}", i.info.server_version),
            "api": "docker.sock",
            "uid": i.info.swarm.cluster.as_ref().map_or("", |c| c.id.as_str()),
        }))),
        ..Default::default()
    }];

    // ---- nodes -> hosts
    let mut nodes: Vec<&SwarmNode> = i.nodes.iter().collect();
    nodes.sort_by(|a, b| a.description.hostname.cmp(&b.description.hostname));
    let mut host_name: HashMap<&str, &str> = HashMap::new();
    let (mut managers, mut workers) = (Vec::new(), Vec::new());
    for n in nodes {
        let (own, reason) = node_state(n);
        host_name.insert(&n.id, &n.description.hostname);
        let d = &n.description;
        out.push(PbNode {
            id: host_id(&n.id),
            kind: NodeKind::Host.into(),
            name: d.hostname.clone(),
            parent: Some(cid.to_string()),
            provider: Provider::Swarm.into(),
            own: own.into(),
            reason,
            since: i.now_ms,
            meta: Some(struct_from_json(json!({
                "ip": n.status.addr,
                "role": n.spec.role,
                "osType": d.platform.os.to_lowercase(),
                "arch": normalize_arch(&d.platform.architecture),
                "vcpu": d.resources.nano_cpus as f64 / 1e9,
                "ram": d.resources.memory_bytes as f64 / (1u64 << 30) as f64,
                "os": format!("{}/{} · Docker {}", d.platform.os, d.platform.architecture, d.engine.engine_version),
            }))),
            ..Default::default()
        });
        if n.spec.role == "manager" {
            managers.push(n.id.as_str())
        } else {
            workers.push(n.id.as_str())
        }
    }

    // ---- tasks -> workloads: group by slot (replicated) or by node (global); the one meant to run (or about to) is the current instance
    let services: HashMap<&str, &SwarmService> =
        i.services.iter().map(|s| (s.id.as_str(), s)).collect();
    type Key = (String, String);
    let mut current: HashMap<Key, &SwarmTask> = HashMap::new();
    let mut failed: HashMap<Key, u32> = HashMap::new();
    let mut last_err: HashMap<Key, &str> = HashMap::new();
    for t in i.tasks {
        let Some(service) = services.get(t.service_id.as_str()) else {
            continue;
        };
        let global = service.spec.mode.global.is_some();
        let slot = if global || t.slot == 0 {
            format!("n{}", t.node_id)
        } else {
            t.slot.to_string()
        };
        let key = (t.service_id.clone(), slot);
        if t.desired_state == "running" || t.desired_state == "ready" {
            // "ready": swarm is waiting out the restart delay
            if current
                .get(&key)
                .is_none_or(|c| t.updated_at > c.updated_at)
            {
                current.insert(key, t);
            }
        } else if matches!(
            TaskState::parse(&t.status.state),
            TaskState::Failed | TaskState::Rejected
        ) && failed_recently(t, i.now_ms)
        {
            *failed.entry(key.clone()).or_default() += 1;
            let message = first_non_empty(&[&t.status.err, &t.status.message]);
            if !message.is_empty() {
                last_err.insert(key, message);
            }
        }
    }
    let mut keys: Vec<&Key> = current.keys().collect();
    keys.sort_by_key(|(service, slot)| format!("{service}{slot}"));
    let mut on_network: HashMap<&str, Vec<String>> = HashMap::new(); // network id -> the workloads on it
    for key in keys {
        let (t, service) = (current[key], services[key.0.as_str()]);
        let Some(host) = host_name.get(t.node_id.as_str()) else {
            continue;
        }; // not placed on a node yet
        let failures = failed.get(key).copied().unwrap_or(0);
        let (own, reason) = task_state(t, failures, last_err.get(key).copied().unwrap_or_default());
        let global = service.spec.mode.global.is_some() || t.slot == 0;
        let (name, kind) = if global {
            (format!("{}.{host}", service.spec.name), "Task (global)")
        } else {
            (format!("{}.{}", service.spec.name, t.slot), "Task")
        };
        let stack = service
            .spec
            .labels
            .as_ref()
            .and_then(|l| l.get("com.docker.stack.namespace"))
            .filter(|s| !s.is_empty())
            .map_or("—", String::as_str);
        for attachment in t.networks.iter().flatten() {
            on_network
                .entry(attachment.network.id.as_str())
                .or_default()
                .push(format!("{cid}:t:{}", t.id));
        }
        let image = short_image(&t.spec.container_spec.image);
        let container = json!({
            "name": first_non_empty(&[short(&t.status.container_status.container_id, 12), short(&t.id, 12)]).trim(),
            "image": image, "ready": t.status.state == "running", "restarts": failures, "state": t.status.state,
        });
        out.push(PbNode {
            id: format!("{cid}:t:{}", t.id),
            kind: NodeKind::Workload.into(),
            name,
            parent: Some(host_id(&t.node_id)),
            provider: Provider::Swarm.into(),
            own: own.into(),
            reason,
            since: i.now_ms,
            meta: Some(struct_from_json(json!({"type": kind, "image": image, "ns": stack, "restarts": failures, "containers": [container], "phase": t.status.state, "taskID": t.id}))),
            ..Default::default()
        });
    }

    // ---- networks: the ones you defined, with who is on them
    let mut edges = network_nodes(cid, i, &on_network, &mut out);

    // ---- manager -> worker links (Swarm exposes no traffic figures)
    edges.extend(
        managers
            .iter()
            .flat_map(|m| workers.iter().map(move |w| (*m, *w)))
            .map(|(m, w)| Edge {
                id: format!("{}>{}", host_id(m), host_id(w)),
                from: host_id(m),
                to: host_id(w),
                r#type: EdgeType::Control.into(),
                ..Default::default()
            }),
    );
    (out, edges)
}

/// A network is drawn when it is swarm-wide, is not the routing mesh and has someone on it: an overlay you made, or a macvlan/ipvlan that puts
/// services on your LAN or a VLAN. `bridge`, `host`, `none` and `docker_gwbridge` are per node plumbing and never show up here.
fn network_nodes(
    cid: &str,
    i: &Inputs<'_>,
    on_network: &HashMap<&str, Vec<String>>,
    out: &mut Vec<PbNode>,
) -> Vec<Edge> {
    let mut networks: Vec<&SwarmNetwork> = i
        .networks
        .iter()
        .filter(|n| n.scope == "swarm" && !n.ingress)
        .filter(|n| on_network.contains_key(n.id.as_str()))
        .collect();
    networks.sort_by(|a, b| a.name.cmp(&b.name));
    let mut edges = Vec::new();
    for n in networks {
        let members = &on_network[n.id.as_str()];
        let id = format!("{cid}:net:{}", n.id);
        let subnet = n
            .ipam
            .config
            .iter()
            .flatten()
            .map(|c| c.subnet.as_str())
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        let stack = n
            .labels
            .as_ref()
            .and_then(|l| l.get("com.docker.stack.namespace"))
            .filter(|s| !s.is_empty())
            .map_or("—", String::as_str);
        let encrypted = n
            .options
            .as_ref()
            .is_some_and(|o| o.contains_key("encrypted"));
        out.push(PbNode {
            id: id.clone(),
            kind: NodeKind::Network.into(),
            name: n.name.clone(),
            parent: Some(cid.to_string()),
            provider: Provider::Swarm.into(),
            own: Own::Ok.into(),
            since: i.now_ms,
            meta: Some(struct_from_json(json!({
                "type": format!("Network ({})", n.driver), "netKind": n.driver, "subnet": subnet, "ns": stack,
                "internal": n.internal, "encrypted": encrypted, "members": members.len(),
            }))),
            ..Default::default()
        });
        for m in members {
            edges.push(Edge {
                id: format!("{id}>{m}"),
                from: id.clone(),
                to: m.clone(),
                r#type: EdgeType::Route.into(),
                ..Default::default()
            });
        }
    }
    edges
}

#[cfg(test)]
#[path = "../tests/unit/topology.rs"]
mod tests;
