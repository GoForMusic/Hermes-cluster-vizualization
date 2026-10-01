//! The named volumes of this node, as swarm nodes: only the node that has a volume can measure it, so each agent reports its own and the hub
//! adds them to the topology (a `Contribution`). Docker knows how much a volume takes, but not how much it may take: a local volume has no
//! limit of its own, so no size is reported and the hub shows what is used, without a percentage and without an alert on it.
//!
//! `VolumeWatch` is `metrics::run`'s handle on all this: it measures now and then (walking a volume's files is not done on every
//! sample), and only contributes to the topology when the set of volumes or what they say actually changed.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use hermes_agentkit::SharedSink;
use hermes_proto::v1::{Node as PbNode, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use serde_json::json;
use tracing::warn;

use crate::docker::{LocalContainer, SystemDf, VolumeInfo};
use crate::engine::Engine;

const GIB: f64 = (1u64 << 30) as f64;
/// The engine walks a volume's files to say how big it is: not on every sample.
const VOLUMES_EVERY: Duration = Duration::from_secs(30);

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// A volume Docker made up for a container that did not name one: 64 hex digits. Nobody named it, and nobody wants to look at it.
pub fn is_anonymous(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Which services use which volume, from the swarm containers on this node.
pub fn mounted_by(containers: &[LocalContainer]) -> HashMap<String, Vec<String>> {
    let mut by_volume: HashMap<String, BTreeSet<String>> = HashMap::new();
    for c in containers {
        let Some(service) = c
            .labels
            .as_ref()
            .and_then(|l| l.get("com.docker.swarm.service.name"))
        else {
            continue;
        };
        for m in c
            .mounts
            .iter()
            .flatten()
            .filter(|m| m.kind == "volume" && !m.name.is_empty())
        {
            by_volume
                .entry(m.name.clone())
                .or_default()
                .insert(service.clone());
        }
    }
    by_volume
        .into_iter()
        .map(|(volume, services)| (volume, services.into_iter().collect()))
        .collect()
}

/// The volumes of this node as nodes, and how much of each is used (GiB), by node id.
pub fn build(
    source_id: &str,
    node_id: &str,
    volumes: &[VolumeInfo],
    mounted: &HashMap<String, Vec<String>>,
    now_ms: i64,
) -> (Vec<PbNode>, HashMap<String, f64>) {
    let mut list: Vec<&VolumeInfo> = volumes
        .iter()
        .filter(|v| !is_anonymous(&v.name) && v.usage.as_ref().is_some_and(|u| u.size >= 0))
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    let mut nodes = Vec::new();
    let mut used = HashMap::new();
    for v in list {
        let id = format!("{source_id}:v:{node_id}:{}", v.name);
        let gib = v.usage.as_ref().map_or(0.0, |u| u.size as f64 / GIB);
        let stack = v
            .labels
            .as_ref()
            .and_then(|l| l.get("com.docker.stack.namespace"))
            .filter(|s| !s.is_empty())
            .map_or("—", String::as_str);
        used.insert(id.clone(), gib);
        nodes.push(PbNode {
            id,
            kind: NodeKind::Volume.into(),
            name: v.name.clone(),
            parent: Some(format!("{source_id}:n:{node_id}")),
            provider: Provider::Swarm.into(),
            own: Own::Ok.into(),
            since: now_ms,
            m: HashMap::from([("used".to_string(), gib)]),
            meta: Some(struct_from_json(json!({
                "ns": stack,
                "sc": v.driver,
                "mountedBy": mounted.get(&v.name).map(|s| s.join(", ")).unwrap_or_default(),
                "usageKnown": true,
            }))),
            ..Default::default()
        });
    }
    (nodes, used)
}

/// The volumes of this node. They are measured now and then, contributed to the topology when the set of them or what it says changes, and
/// their usage goes out with every sample.
#[derive(Default)]
pub(crate) struct VolumeWatch {
    measured: Option<Instant>,
    nodes: Vec<PbNode>,
    /// node id -> GiB used
    pub(crate) used: HashMap<String, f64>,
    /// what was last contributed, without the numbers (those travel as metrics)
    contributed: Option<Vec<PbNode>>,
}

impl VolumeWatch {
    pub(crate) async fn refresh(
        &mut self,
        engine: &Engine,
        source_id: &str,
        node_id: &str,
        containers: &[LocalContainer],
    ) {
        if self.measured.is_some_and(|t| t.elapsed() < VOLUMES_EVERY) {
            return;
        }
        self.measured = Some(Instant::now());
        let df: SystemDf = match engine.get("/system/df?type=volume").await {
            Ok(df) => df,
            Err(e) => {
                warn!("swarm: measuring the volumes: {e:#}");
                return;
            }
        };
        (self.nodes, self.used) = build(
            source_id,
            node_id,
            df.volumes.as_deref().unwrap_or_default(),
            &mounted_by(containers),
            now_ms(),
        );
    }

    pub(crate) fn publish(&mut self, sink: &SharedSink) {
        let shape: Vec<PbNode> = self
            .nodes
            .iter()
            .map(|n| PbNode {
                m: HashMap::new(),
                ..n.clone()
            })
            .collect();
        let nothing_yet = self.contributed.is_none() && shape.is_empty();
        if !nothing_yet && self.contributed.as_ref() != Some(&shape) {
            sink.contribute(self.nodes.clone());
            self.contributed = Some(shape);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/volumes.rs"]
mod tests;
