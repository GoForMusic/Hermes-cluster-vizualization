//! From what the kernel saw (`Flow`: addresses and rates, per node) to numbers on the links of the map. The agent knows addresses, the hub
//! knows what they are: a pod's `podIP`, a Service's `ip`, a node's `ip`. Only links that already exist get a rate, so what is drawn is what the
//! API said is there; a flow that matches no link is left out rather than invented.
//!
//! * Service to the pod that answered: the connection asked for the Service address and the pod behind it answered (conntrack shows both).
//! * Outside to a Service: the asker is an address the cluster does not know, and it reached a load balancer or node port Service.

use std::collections::{HashMap, HashSet};

use hermes_proto::v1 as pb;
use serde_json::Value;

use crate::model::{Edge, FlowLine, Node};

/// The `ip` -> node id of everything that has an address of its own.
fn addresses(nodes: &[Node]) -> HashMap<&str, &Node> {
    let mut out = HashMap::new();
    for n in nodes {
        let key = match n.kind.as_str() {
            "workload" => "podIP",
            "network" | "host" => "ip",
            _ => continue,
        };
        if let Some(ip) = n
            .meta
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|ip| !ip.is_empty())
        {
            out.entry(ip).or_insert(n);
        }
    }
    out
}

/// The Mbit/s of every route link that carries traffic, from the reports of the agents of one source (`nodes` and `edges` are that source's).
///
/// The same connection is seen by every node it crosses: reports are merged by flow first (the largest wins), then added up on the links.
pub fn edge_rates(nodes: &[Node], edges: &[Edge], reports: &[&[pb::Flow]]) -> HashMap<String, f64> {
    let known = addresses(nodes);
    let edge_ids: HashSet<&str> = edges.iter().map(|e| e.id.as_str()).collect();
    let outside = nodes.iter().find(|n| {
        n.kind == "network" && n.meta.get("netKind").and_then(|v| v.as_str()) == Some("outside")
    });

    let mut merged: HashMap<(&str, &str, &str, u32, &str), f64> = HashMap::new();
    for f in reports.iter().flat_map(|r| r.iter()) {
        let total = f.out_mbps + f.in_mbps;
        let slot = merged
            .entry((&f.src, &f.dst, &f.served_by, f.port, &f.proto))
            .or_insert(0.0);
        *slot = slot.max(total);
    }

    let mut rates: HashMap<String, f64> = HashMap::new();
    let mut add = |id: String, mbps: f64| {
        if edge_ids.contains(id.as_str()) {
            *rates.entry(id).or_insert(0.0) += mbps;
        }
    };
    for (&(src, dst, served_by, _, _), &mbps) in &merged {
        let Some(target) = known.get(dst).filter(|n| n.kind == "network") else {
            // a load balancer or node port is reached on an address that is not the Service's own: find the Service by the pod that answered
            if let (Some(out), Some(pod)) = (outside, known.get(served_by))
                && !known.contains_key(src)
            {
                let via = edges
                    .iter()
                    .filter(|e| e.to == pod.id)
                    .find(|e| edge_ids.contains(format!("{}>{}", out.id, e.from).as_str()));
                if let Some(e) = via {
                    add(format!("{}>{}", out.id, e.from), mbps);
                    add(e.id.clone(), mbps);
                }
            }
            continue;
        };
        if let Some(pod) = known.get(served_by).filter(|n| n.kind == "workload") {
            add(format!("{}>{}", target.id, pod.id), mbps);
        }
        if !known.contains_key(src)
            && let Some(out) = outside
        {
            add(format!("{}>{}", out.id, target.id), mbps);
        }
    }
    ingress_rates(nodes, edges, &known, &merged, &edge_ids, &mut rates);
    rates
}

/// Traffic through an ingress controller, put on the links of the Ingress: what the controller pods exchange with the pods behind a Service that
/// an Ingress sends to is what came in through that Ingress, and the same amount is what came in from outside for it. It is a deduction (the
/// controller may also serve something else), which is why the map marks these numbers as approximate.
fn ingress_rates(
    nodes: &[Node],
    edges: &[Edge],
    known: &HashMap<&str, &Node>,
    merged: &HashMap<(&str, &str, &str, u32, &str), f64>,
    edge_ids: &HashSet<&str>,
    rates: &mut HashMap<String, f64>,
) {
    let controllers: HashSet<&str> = nodes
        .iter()
        .filter(|n| {
            n.kind == "workload"
                && n.meta.get("ingressController").and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|n| n.meta.get("podIP").and_then(Value::as_str))
        .collect();
    if controllers.is_empty() {
        return;
    }
    let kind_of = |id: &str| {
        nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| n.meta.get("netKind"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // the Services that lead to a pod, and the Ingresses that lead to a Service
    let mut services_of: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut ingresses_of: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        match kind_of(&e.from).as_str() {
            "service" | "nodeport" | "loadbalancer" => {
                services_of.entry(&e.to).or_default().push(&e.from)
            }
            "ingress" => ingresses_of.entry(&e.to).or_default().push(&e.from),
            _ => {}
        }
    }
    let outside_edge = |ing: &str| {
        edges
            .iter()
            .find(|e| e.to == ing && kind_of(&e.from) == "outside")
            .map(|e| e.id.as_str())
    };
    for ((src, dst, served_by, _, _), mbps) in merged {
        if !controllers.contains(src) {
            continue;
        }
        let target = known
            .get(if served_by.is_empty() { dst } else { served_by })
            .filter(|n| n.kind == "workload");
        for svc in target
            .into_iter()
            .flat_map(|p| services_of.get(p.id.as_str()).into_iter().flatten())
        {
            for ing in ingresses_of.get(svc).into_iter().flatten() {
                for id in [
                    Some(format!("{ing}>{svc}")),
                    outside_edge(ing).map(String::from),
                ]
                .into_iter()
                .flatten()
                {
                    if edge_ids.contains(id.as_str()) {
                        *rates.entry(id).or_insert(0.0) += mbps;
                    }
                }
            }
        }
    }
}

/// The busiest connections of a source, biggest first, for the list on the wallboard. Ends the cluster knows are told by node id, the others by address.
pub fn top_flows(nodes: &[Node], reports: &[&[pb::Flow]], limit: usize) -> Vec<FlowLine> {
    let known = addresses(nodes);
    let mut merged: HashMap<(&str, &str, &str, u32, &str), f64> = HashMap::new();
    for f in reports.iter().flat_map(|r| r.iter()) {
        let slot = merged
            .entry((&f.src, &f.dst, &f.served_by, f.port, &f.proto))
            .or_insert(0.0);
        *slot = slot.max(f.out_mbps + f.in_mbps);
    }
    let name = |ip: &str| {
        known
            .get(ip)
            .map_or_else(|| ip.to_string(), |n| n.id.clone())
    };
    let lines: Vec<FlowLine> = merged
        .into_iter()
        .filter(|(_, mbps)| *mbps >= 0.01)
        .map(|((src, dst, served_by, port, _), mbps)| FlowLine {
            src: name(src),
            dst: name(dst),
            via: known
                .get(served_by)
                .map(|n| n.id.clone())
                .unwrap_or_default(),
            port,
            mbps,
            external: !known.contains_key(src),
        })
        .collect();
    // one line for each pair: the pod that answered is named only when there was just one
    let mut pairs: HashMap<(String, String), FlowLine> = HashMap::new();
    for l in lines {
        match pairs.entry((l.src.clone(), l.dst.clone())) {
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(l);
            }
            std::collections::hash_map::Entry::Occupied(mut o) => {
                let sum = o.get_mut();
                if sum.via != l.via {
                    sum.via.clear();
                }
                if l.mbps > sum.mbps {
                    sum.port = l.port;
                }
                sum.mbps += l.mbps;
            }
        }
    }
    let mut lines: Vec<FlowLine> = pairs.into_values().collect();
    lines.sort_by(|a, b| {
        b.mbps
            .total_cmp(&a.mbps)
            .then_with(|| (&a.src, &a.dst, a.port).cmp(&(&b.src, &b.dst, b.port)))
    });
    lines.truncate(limit);
    lines
}

#[cfg(test)]
#[path = "../../tests/unit/flows.rs"]
mod tests;
