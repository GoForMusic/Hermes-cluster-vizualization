//! A development agent: replays a topology in the browsers' JSON (`examples/seed.json`, or a snapshot saved from `/api/snapshot`)
//! through the real gRPC uplink, with numbers that drift, so the hub and the web app can be looked at without a cluster.
//!
//!     HUB_URL=http://127.0.0.1:8780 SOURCE_ID=s… TOKEN=… cargo run -p hermes-hub --example replay -- infraviz-server/examples/seed.json

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use hermes_agentkit::{Config, ISink, Uplink};
use hermes_proto::v1::{Edge, EdgeType, Node, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use serde_json::Value;

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or_default()
}

fn own(name: &str) -> Own {
    match name {
        "ok" => Own::Ok,
        "warn" => Own::Warn,
        "crit" => Own::Crit,
        _ => Own::Unspecified,
    }
}

fn node(v: &Value) -> Node {
    let kind = match text(v, "kind") {
        "cluster" => NodeKind::Cluster,
        "host" => NodeKind::Host,
        "workload" => NodeKind::Workload,
        "volume" => NodeKind::Volume,
        _ => NodeKind::Unspecified,
    };
    let provider = match text(v, "provider") {
        "kubernetes" => Provider::Kubernetes,
        "swarm" => Provider::Swarm,
        "docker" => Provider::Docker,
        "nomad" => Provider::Nomad,
        "storage" => Provider::Storage,
        _ => Provider::Unspecified,
    };
    Node {
        id: text(v, "id").into(),
        kind: kind.into(),
        name: text(v, "name").into(),
        parent: v["parent"].as_str().map(Into::into),
        provider: provider.into(),
        own: own(text(v, "own")).into(),
        reason: text(v, "reason").into(),
        since: v["since"].as_i64().unwrap_or_default(),
        m: v["m"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, x)| Some((k.clone(), x.as_f64()?)))
                    .collect()
            })
            .unwrap_or_default(),
        meta: Some(struct_from_json(v["meta"].clone())),
    }
}

fn edge(v: &Value) -> Edge {
    Edge {
        id: text(v, "id").into(),
        from: text(v, "from").into(),
        to: text(v, "to").into(),
        base: v["base"].as_f64().unwrap_or_default(),
        mbps: v["mbps"].as_f64().unwrap_or_default(),
        r#type: if text(v, "type") == "control" {
            EdgeType::Control
        } else {
            EdgeType::Traffic
        }
        .into(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: replay <topology.json>")?;
    let seed: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    let nodes: Vec<Node> = seed["nodes"]
        .as_array()
        .context("no nodes")?
        .iter()
        .map(node)
        .collect();
    let edges: Vec<Edge> = seed["edges"]
        .as_array()
        .map(|e| e.iter().map(edge).collect())
        .unwrap_or_default();

    let cfg = Config::from_env("replay", env!("CARGO_PKG_VERSION"))?;
    let link = Uplink::new(&cfg)?;
    link.set_topology(nodes.clone(), edges.clone());
    tokio::spawn(link.clone().run());
    println!(
        "replaying {} nodes and {} edges to {}",
        nodes.len(),
        edges.len(),
        cfg.hub_url
    );

    let link: Arc<dyn ISink> = Arc::new(link);
    let mut tick = 0u32;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        tick += 1;
        // a slow wave around each node's own numbers
        let phase = f64::from(tick) / 6.0;
        let metrics: HashMap<String, HashMap<String, f64>> = nodes
            .iter()
            .filter(|n| n.kind() != NodeKind::Cluster && !n.m.is_empty())
            .enumerate()
            .map(|(i, n)| {
                let wave = (phase + i as f64).sin();
                (
                    n.id.clone(),
                    n.m.iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                if k == "used" {
                                    *v
                                } else {
                                    (v + wave * 6.0).clamp(1.0, 99.0)
                                },
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        let traffic = edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                (
                    e.id.clone(),
                    (e.base * (1.0 + 0.4 * (phase + i as f64).sin())).max(0.0),
                )
            })
            .collect();
        link.metrics(metrics, traffic);
    }
}
