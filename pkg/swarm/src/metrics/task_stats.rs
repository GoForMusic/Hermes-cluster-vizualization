//! Fetches every swarm task's container stats on this node, a few at a time, and turns them into the metrics `metrics::run` sends —
//! plus telling which network a container's traffic counters belong to, since the Engine names them by interface, not by network.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use hermes_agentkit::netrate::{Sample, Tracker};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::stats::{
    ContainerStats, NetStats, cpu_milli, cpu_milli_windows, mem_mib, mem_mib_windows, parse_ms,
};
use crate::docker::{ContainerNetwork, LocalContainer};
use crate::engine::Engine;

/// A few stats calls at a time: each takes about a second.
const PARALLEL: usize = 8;

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// Measures the swarm containers running on this node, a few at a time.
pub(crate) async fn task_stats(
    engine: &Arc<Engine>,
    list: &[LocalContainer],
    source_id: &str,
    ncpu: f64,
    mem_total_mib: f64,
    windows: bool,
    (net, net_of): (&mut Tracker, &mut Tracker),
) -> (HashMap<String, HashMap<String, f64>>, HashMap<String, f64>) {
    let permits = Arc::new(Semaphore::new(PARALLEL));
    let mut jobs = JoinSet::new();
    let mut live = HashSet::new();
    let mut attached: HashMap<String, HashMap<String, ContainerNetwork>> = HashMap::new();
    for c in list {
        let id = c.id.clone();
        let Some(task) = c
            .labels
            .as_ref()
            .and_then(|l| l.get("com.docker.swarm.task.id"))
            .filter(|t| !t.is_empty())
            .cloned()
        else {
            continue;
        };
        live.insert(task.clone());
        if let Some(nets) = c.network_settings.as_ref().and_then(|n| n.networks.clone()) {
            attached.insert(task.clone(), nets);
        }
        let (engine, permits) = (engine.clone(), permits.clone());
        jobs.spawn(async move {
            let _permit = permits.acquire_owned().await.ok()?;
            let stats = tokio::time::timeout(
                Duration::from_secs(6),
                engine.get::<ContainerStats>(&format!("/containers/{id}/stats?stream=false")),
            )
            .await
            .ok()?
            .ok()?;
            Some((task, stats))
        });
    }

    let mut out = HashMap::new();
    let mut links = HashMap::new(); // network -> task: what the task moved on that network, Mbit/s
    let mut live_links = HashSet::new();
    while let Some(done) = jobs.join_next().await {
        let Ok(Some((task, s))) = done else { continue };
        let (milli, mem) = if windows {
            (cpu_milli_windows(&s), mem_mib_windows(&s))
        } else {
            (cpu_milli(&s), mem_mib(&s))
        };
        let mut m = HashMap::from([
            ("cpuMilli".to_string(), milli),
            ("memMiB".to_string(), mem),
            ("cpu".to_string(), milli / (ncpu * 1000.0) * 100.0),
        ]);
        if mem_total_mib > 0.0 {
            m.insert("mem".to_string(), mem / mem_total_mib * 100.0);
        }
        if !s.networks.is_empty() {
            // summed over the container's interfaces; left out when the engine reports none
            let (rx, tx) = s
                .networks
                .values()
                .fold((0, 0), |(rx, tx), n| (rx + n.rx_bytes, tx + n.tx_bytes));
            let at_ms = parse_ms(&s.read).unwrap_or_else(now_ms);
            if let Some((r, t)) = net.update(&task, Sample { rx, tx, at_ms }) {
                m.insert("rxMbps".to_string(), r);
                m.insert("txMbps".to_string(), t);
            }
        }
        if let Some((network, c)) = attached
            .get(&task)
            .and_then(|a| overlay_counters(a, &s.networks))
        {
            let key = format!("{task}|{network}");
            live_links.insert(key.clone());
            let at_ms = parse_ms(&s.read).unwrap_or_else(now_ms);
            let sample = Sample {
                rx: c.rx_bytes,
                tx: c.tx_bytes,
                at_ms,
            };
            if let Some((r, t)) = net_of.update(&key, sample) {
                links.insert(
                    format!("{source_id}:net:{network}>{source_id}:t:{task}"),
                    r + t,
                );
            }
        }
        out.insert(format!("{source_id}:t:{task}"), m);
    }
    net.keep(&live);
    net_of.keep(&live_links);
    (out, links)
}

/// Which network a container's traffic counters belong to, when that can be told. The engine names the counters by the container's own
/// interfaces (`eth0`, `eth1`...) and not by network. A task on exactly one network (not counting the routing mesh, `ingress`) has that network
/// on `eth0`, and `eth1` is its way out (`docker_gwbridge`); with more than one network there is no telling which is which, so none is claimed.
pub fn overlay_counters<'a>(
    attached: &'a HashMap<String, ContainerNetwork>,
    interfaces: &'a HashMap<String, NetStats>,
) -> Option<(&'a str, &'a NetStats)> {
    let mut own = attached
        .iter()
        .filter(|(name, n)| name.as_str() != "ingress" && !n.id.is_empty());
    let (_, only) = own.next()?;
    if own.next().is_some() {
        return None;
    }
    let counters = interfaces.get("eth0").or_else(|| {
        (interfaces.len() == 1)
            .then(|| interfaces.values().next())
            .flatten()
    })?;
    Some((only.id.as_str(), counters))
}

#[cfg(test)]
#[path = "../../tests/unit/task_stats.rs"]
mod tests;
