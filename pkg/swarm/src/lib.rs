//! The Docker Swarm collector, shared by the Linux and the Windows agent. It talks to the Docker Engine API of the node it runs on. Every node
//! reports its own CPU and memory and that of the swarm tasks running there; a manager also reports the topology (nodes, services, tasks),
//! because the Engine API only answers swarm questions on a manager.
//!
//! What differs per operating system is passed in: how to reach the engine (a unix socket or a named pipe: [`engine::connector_for`]) and how to
//! read the host (a [`ISampler`]).

pub mod docker;
pub mod engine;
pub mod metrics;
mod state;
pub mod topology;
pub mod upgrade;
pub mod volumes;

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use hermes_agentkit::Config;
use hermes_agentkit::SharedSink;
use hermes_agentkit::differ::Differ;
use hermes_agentkit::host::ISampler;
use hermes_proto::v1::{CollectorState, NodeKind};

use docker::{EngineInfo, SwarmNetwork, SwarmNode, SwarmService, SwarmTask};
use engine::{Engine, connector_for};
use topology::Inputs;

const POLL_EVERY: Duration = Duration::from_secs(3);
/// This collector's own meta fields that change while the topology stays the same: a task's restart count and state, and its container.
const RUNTIME_META: &[&str] = &["restarts", "phase", "containers"];

/// Wires up a Swarm agent's `main`: `Config` and `DOCKER_SOCKET` (or `default_socket` when unset) from the environment, then reports to
/// the hub until the process is asked to stop, restarting the collector on failure. This is the one thing that differs between the
/// Linux and the Windows agent — the rest of `main` is identical, so both call this instead of repeating it.
///
/// `new_sampler` is called once per connection attempt, the same as the engine itself: a fresh instance on every reconnect, so it must
/// not carry over state (like a CPU tracker's previous reading) from one attempt to the next.
pub async fn run_agent(
    version: &str,
    default_socket: &str,
    new_sampler: impl Fn() -> Box<dyn ISampler> + Send + Sync + 'static,
) -> Result<()> {
    let cfg = Config::from_env("swarm", version)?;
    // `UPGRADES=1` (set by the install manifest when the admin allowed it): the hub may ask this agent to change the stack's image
    let upgrades = std::env::var("UPGRADES").is_ok_and(|v| !v.is_empty() && v != "0");
    let socket = std::env::var("DOCKER_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_socket.to_string());
    let (source_id, source_name) = (cfg.source_id.clone(), cfg.source_name.clone());
    let upgrader: Option<hermes_agentkit::SharedUpgrader> = upgrades.then(|| {
        Arc::new(upgrade::SwarmUpgrader::new(Arc::new(Engine::new(
            connector_for(&socket),
        )))) as _
    });
    hermes_agentkit::run_with(cfg, upgrader, move |sink: SharedSink| {
        let (source_id, source_name) = (source_id.clone(), source_name.clone());
        let engine = Arc::new(Engine::new(connector_for(&socket)));
        let sampler = new_sampler();
        async move { run(&source_id, &source_name, engine, sampler, sink).await }
    })
    .await
}

/// Watches the swarm through the engine until the collector fails (the caller restarts it).
pub async fn run(
    source_id: &str,
    source_name: &str,
    engine: Arc<Engine>,
    sampler: Box<dyn ISampler>,
    sink: SharedSink,
) -> Result<()> {
    let info: EngineInfo = engine
        .get("/info")
        .await
        .context("cannot reach the Docker engine")?;
    if info.swarm.node_id.is_empty() {
        bail!("this Docker engine is not part of a swarm");
    }
    let measuring = metrics::run(source_id, engine.clone(), &info, sampler, sink.clone());
    if info.swarm.control_available && info.swarm.cluster.is_some() {
        tokio::select! {
            r = measuring => r,
            r = watch_topology(source_id, source_name, &engine, &info, sink) => r,
        }
    } else {
        sink.report(
            CollectorState::Connected,
            "worker node: reporting CPU and memory only",
        );
        measuring.await
    }
}

async fn watch_topology(
    source_id: &str,
    source_name: &str,
    engine: &Engine,
    info: &EngineInfo,
    sink: SharedSink,
) -> Result<()> {
    let mut differ = Differ::new(sink.clone(), RUNTIME_META);
    let mut tick = tokio::time::interval(POLL_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let (nodes, services, tasks, networks): (
            Vec<SwarmNode>,
            Vec<SwarmService>,
            Vec<SwarmTask>,
            Vec<SwarmNetwork>,
        ) = tokio::try_join!(
            engine.get("/nodes"),
            engine.get("/services"),
            engine.get("/tasks"),
            engine.get("/networks")
        )?;
        let now_ms = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis()),
        )
        .unwrap_or(0);
        let (built, edges) = topology::build(&Inputs {
            source_id,
            source_name,
            info,
            nodes: &nodes,
            services: &services,
            tasks: &tasks,
            networks: &networks,
            now_ms,
        });
        let workloads = built
            .iter()
            .filter(|n| n.kind() == NodeKind::Workload)
            .count();
        if differ.apply(built, edges) {
            sink.report(
                CollectorState::Connected,
                &format!(
                    "{} nodes · {} services · {workloads} tasks",
                    nodes.len(),
                    services.len()
                ),
            );
        }
    }
}
