//! What a node measures about itself and about the swarm tasks that run on it. Only a node can measure itself: a manager cannot read other
//! nodes' containers. So the agent runs on every node (a global service); each one reports its own CPU and memory and that of its tasks.
//!
//! Orchestration only: the container stats DTOs and their pure CPU/memory math live in `stats.rs`, the concurrent per-container fetch in
//! `task_stats.rs`, the volume watch in `volumes.rs` (it is an entity concern, not a metrics one).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hermes_agentkit::SharedSink;
use hermes_agentkit::host::ISampler;
use hermes_agentkit::netrate::Tracker;
use tracing::warn;

use crate::docker::{EngineInfo, LocalContainer};
use crate::engine::{Engine, pct_encode};
use crate::volumes::VolumeWatch;

pub(crate) mod stats;
mod task_stats;

const EVERY: Duration = Duration::from_secs(5);
const MIB: f64 = (1u64 << 20) as f64;

/// Reports this node's CPU and memory and its swarm tasks' usage until the task is dropped.
pub async fn run(
    source_id: &str,
    engine: Arc<Engine>,
    info: &EngineInfo,
    mut sampler: Box<dyn ISampler>,
    sink: SharedSink,
) -> Result<()> {
    let host_id = format!("{source_id}:n:{}", info.swarm.node_id);
    let ncpu = info.ncpu.max(1) as f64;
    let mut mem_total = info.mem_total as f64 / MIB;
    let windows = info.os_type == "windows";
    let mut net = Tracker::new();
    let mut net_of = Tracker::new(); // the traffic of a task on its network
    let mut volumes = VolumeWatch::default();

    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let mut out: HashMap<String, HashMap<String, f64>> = HashMap::new();

        if let Ok(hs) = sampler.sample() {
            let mut m = HashMap::new();
            if let Some(cpu) = hs.cpu {
                m.insert("cpu".to_string(), cpu);
                m.insert("cpuMilli".to_string(), cpu * ncpu * 10.0);
            }
            if hs.mem_total_mib > 0.0 {
                m.insert(
                    "mem".to_string(),
                    hs.mem_used_mib / hs.mem_total_mib * 100.0,
                );
                m.insert("memMiB".to_string(), hs.mem_used_mib);
                mem_total = hs.mem_total_mib;
            }
            if !m.is_empty() {
                out.insert(host_id.clone(), m);
            }
        }

        let containers = local_containers(&engine).await;
        volumes
            .refresh(&engine, source_id, &info.swarm.node_id, &containers)
            .await;
        volumes.publish(&sink);
        for (id, used) in &volumes.used {
            out.insert(id.clone(), HashMap::from([("used".to_string(), *used)]));
        }

        let mut ids = Vec::new();
        let (tasks, links) = task_stats::task_stats(
            &engine,
            &containers,
            source_id,
            ncpu,
            mem_total,
            windows,
            (&mut net, &mut net_of),
        )
        .await;
        for (id, m) in tasks {
            ids.push(id.clone()); // a task whose container answered its stats call is running here
            out.insert(id, m);
        }
        sink.alive(ids);
        if !out.is_empty() || !links.is_empty() {
            sink.metrics(out, links);
        }
    }
}

/// The swarm containers on this node (the ones that carry a task id).
async fn local_containers(engine: &Engine) -> Vec<LocalContainer> {
    let filter = pct_encode(r#"{"label":["com.docker.swarm.task.id"]}"#);
    match engine
        .get(&format!("/containers/json?filters={filter}"))
        .await
    {
        Ok(list) => list,
        Err(e) => {
            warn!("swarm: listing local containers: {e:#}");
            Vec::new()
        }
    }
}
