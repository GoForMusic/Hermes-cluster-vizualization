//! The kubelet's stats summary: how full a volume is, and what each pod's network counters say. Best effort: needs the `nodes/proxy`
//! permission, and what cannot be measured is left out (unknown), never reported as zero.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use hermes_agentkit::netrate::{Sample, Tracker};
use serde::Deserialize;

pub const GIB: f64 = (1u64 << 30) as f64;

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Summary {
    pub pods: Vec<PodStats>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PodStats {
    pub pod_ref: PodRef,
    pub network: Option<Network>,
    pub volume: Vec<VolumeStats>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PodRef {
    pub name: String,
    pub namespace: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Network {
    pub time: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub interfaces: Vec<Interface>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Interface {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VolumeStats {
    pub used_bytes: f64,
    pub capacity_bytes: f64,
    pub pvc_ref: Option<PodRef>,
}

pub fn parse_summary(json: &str) -> Result<Summary> {
    serde_json::from_str(json).context("the kubelet's stats are not what was expected")
}

/// Does the kubelet's usage figure belong to the volume itself? A volume on a filesystem of its own (a block device, a CSI volume with its
/// own quota) reports a capacity close to what the PVC asked for. A directory on a shared disk (hostPath, local-path) reports the whole
/// disk, and that is not this volume's usage, so it is not used.
pub fn own_filesystem(capacity_bytes: f64, requested_bytes: f64) -> bool {
    if capacity_bytes <= 0.0 || requested_bytes <= 0.0 {
        return false;
    }
    let r = capacity_bytes / requested_bytes;
    (0.85..=1.05).contains(&r) // file system overhead takes a few percent
}

/// What one node's summary says, added to what is known: volume usage in GiB by `ns/pvc`, and the throughput of each pod (Mb/s received and
/// sent) by `ns/pod`. `seen` collects the pods that reported a network, so that the tracker can forget the rest.
pub fn absorb(
    summary: &Summary,
    pvc_size: &HashMap<String, f64>,
    volumes: &mut HashMap<String, f64>,
    net: &mut Tracker,
    seen: &mut HashSet<String>,
    rates: &mut HashMap<String, (f64, f64)>,
) {
    for pod in &summary.pods {
        for v in &pod.volume {
            let Some(pvc) = &v.pvc_ref else { continue };
            let key = format!("{}/{}", pvc.namespace, pvc.name);
            if own_filesystem(v.capacity_bytes, pvc_size.get(&key).copied().unwrap_or(0.0)) {
                volumes.insert(key, v.used_bytes / GIB);
            } else {
                volumes.remove(&key); // a shared disk: the figure would be the node's, not the volume's
            }
        }
        let Some(n) = &pod.network else { continue }; // host-network pods have no network of their own
        let key = format!("{}/{}", pod.pod_ref.namespace, pod.pod_ref.name);
        let (mut rx, mut tx) = (n.rx_bytes, n.tx_bytes);
        if rx == 0 && tx == 0 {
            // some kubelets report only the per-interface list
            for i in &n.interfaces {
                rx += i.rx_bytes;
                tx += i.tx_bytes;
            }
        }
        let Ok(at) = n.time.parse::<jiff::Timestamp>() else {
            continue;
        };
        seen.insert(key.clone());
        if let Some(rate) = net.update(
            &key,
            Sample {
                rx,
                tx,
                at_ms: at.as_millisecond(),
            },
        ) {
            rates.insert(key, rate);
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_kubelet.rs"]
mod tests;
