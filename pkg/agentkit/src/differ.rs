//! Turns "what the world looks like now" into the minimal events for a `ISink`. Every polling collector builds the full node list each
//! round; the `Differ` decides whether that is a new topology or only status, meta and metric changes. Shared, so collectors do not each
//! reimplement it.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use hermes_proto::v1::{Edge, Node, NodeKind};
use prost_types::{Struct, Value, value::Kind};

use crate::SharedSink;

pub struct Differ {
    sink: SharedSink,
    signature: u64,
    last: HashMap<String, Node>,
    /// The collector's own meta fields that change while the topology stays the same (Kubernetes and Swarm both happen to call theirs
    /// `restarts`/`phase`/`containers`/`usageKnown`, but the differ does not know or care what a node's meta holds beyond this list —
    /// a future collector names its own).
    runtime_meta: &'static [&'static str],
}

impl Differ {
    pub fn new(sink: SharedSink, runtime_meta: &'static [&'static str]) -> Self {
        Self {
            sink,
            signature: 0,
            last: HashMap::new(),
            runtime_meta,
        }
    }

    /// Reports what changed since the previous call. Returns true when the topology itself changed (nodes appeared, disappeared or were
    /// renamed) and a full snapshot was sent.
    pub fn apply(&mut self, nodes: Vec<Node>, edges: Vec<Edge>) -> bool {
        let signature = topology_signature(&nodes, &edges);
        let index: HashMap<String, Node> =
            nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();

        if signature != self.signature {
            self.signature = signature;
            self.last = index;
            self.sink.set_topology(nodes, edges);
            return true;
        }

        let mut metrics = HashMap::new();
        for (id, node) in &index {
            if let Some(old) = self.last.get(id) {
                if old.own != node.own || old.reason != node.reason {
                    self.sink.status(id, node.own(), &node.reason);
                }
                if let Some(patch) = meta_patch(old, node, self.runtime_meta) {
                    self.sink.meta(id, patch);
                }
            }
            if node.kind() != NodeKind::Cluster && !node.m.is_empty() {
                metrics.insert(id.clone(), node.m.clone());
            }
        }
        if !metrics.is_empty() {
            self.sink.metrics(metrics, HashMap::new());
        }
        self.last = index;
        false
    }
}

/// Identifies a topology's shape (which nodes and edges exist, and each node's name) without the cost of formatting and joining a
/// string every tick: one hash per item, sorted so the order they came in does not matter, then combined into one value.
fn topology_signature(nodes: &[Node], edges: &[Edge]) -> u64 {
    let mut items: Vec<u64> = nodes
        .iter()
        .map(|n| hash_of(&(&n.id, &n.name)))
        .chain(edges.iter().map(|e| hash_of(&e.id)))
        .collect();
    items.sort_unstable();
    hash_of(&items)
}

fn hash_of(v: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn meta_patch(old: &Node, cur: &Node, runtime_meta: &[&str]) -> Option<Struct> {
    let get = |n: &Node, key: &str| n.meta.as_ref().and_then(|m| m.fields.get(key)).cloned();
    let mut patch = Struct::default();
    for &key in runtime_meta {
        let (a, b) = (get(old, key), get(cur, key));
        if a != b {
            patch.fields.insert(
                key.to_string(),
                b.unwrap_or(Value {
                    kind: Some(Kind::NullValue(0)),
                }),
            );
        }
    }
    (!patch.fields.is_empty()).then_some(patch)
}

#[cfg(test)]
#[path = "../tests/unit/differ.rs"]
mod tests;
