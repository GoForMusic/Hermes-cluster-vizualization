//! metrics-server (`metrics.k8s.io`): what nodes and pods use right now. Best effort: a cluster without it just has no numbers.

use std::collections::HashMap;

use serde::Deserialize;

use super::quantity;

/// What something uses: CPU in millicores, memory in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub milli: f64,
    pub bytes: f64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct List {
    items: Vec<Item>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Item {
    metadata: Meta,
    usage: HashMap<String, String>,
    containers: Vec<Container>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Meta {
    name: String,
    namespace: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Container {
    usage: HashMap<String, String>,
}

fn usage(m: &HashMap<String, String>) -> Usage {
    Usage {
        milli: m.get("cpu").and_then(|c| quantity::milli(c)).unwrap_or(0.0),
        bytes: m
            .get("memory")
            .and_then(|c| quantity::parse(c))
            .unwrap_or(0.0),
    }
}

/// `/apis/metrics.k8s.io/v1beta1/nodes`, by node name.
pub fn parse_nodes(json: &str) -> HashMap<String, Usage> {
    serde_json::from_str::<List>(json)
        .unwrap_or_default()
        .items
        .into_iter()
        .map(|i| (i.metadata.name.clone(), usage(&i.usage)))
        .collect()
}

/// `/apis/metrics.k8s.io/v1beta1/pods`, by `namespace/name`, all the pod's containers added up.
pub fn parse_pods(json: &str) -> HashMap<String, Usage> {
    serde_json::from_str::<List>(json)
        .unwrap_or_default()
        .items
        .into_iter()
        .map(|i| {
            let total =
                i.containers
                    .iter()
                    .map(|c| usage(&c.usage))
                    .fold(Usage::default(), |a, u| Usage {
                        milli: a.milli + u.milli,
                        bytes: a.bytes + u.bytes,
                    });
            (
                format!("{}/{}", i.metadata.namespace, i.metadata.name),
                total,
            )
        })
        .collect()
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_metrics.rs"]
mod tests;
