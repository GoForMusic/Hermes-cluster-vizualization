//! A Docker engine that is not part of a swarm: a plain machine with containers on it, one of several the admin wants on the same map.
//! Nobody here sees the others, so there is no manager to describe the whole: each agent reports its own machine as a host and its own
//! containers as workloads, in a `Contribution`, and the hub puts them together under one cluster of its own.
//!
//! The pure part (`build`, `container_state`) turns what the engine said into nodes; `run` measures and reports.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use hermes_agentkit::host::ISampler;
use hermes_agentkit::netrate::{Sample, Tracker};
use hermes_agentkit::{Config, SharedSink};
use hermes_proto::v1::{CollectorState, Edge, EdgeType, Node as PbNode, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

use crate::docker::{ContainerSummary, EngineInfo, SwarmNetwork};
use crate::engine::{Engine, connector_for};
use crate::metrics::stats::{
    ContainerStats, cpu_milli, cpu_milli_windows, mem_mib, mem_mib_windows, parse_ms,
};
use crate::topology::{normalize_arch, short_image};

const EVERY: Duration = Duration::from_secs(5);
const MIB: f64 = (1u64 << 20) as f64;
const GIB: f64 = (1u64 << 30) as f64;
/// A few stats calls at a time: each takes about a second.
const PARALLEL: usize = 8;
const COMPOSE_PROJECT: &str = "com.docker.compose.project";

/// What tells this machine from the others of the source: the start of the engine's id, or its host name when it has none.
pub fn machine_key(info: &EngineInfo) -> String {
    let id: String = info
        .id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect();
    if id.is_empty() { info.name.clone() } else { id }
}

pub fn host_id(source_id: &str, info: &EngineInfo) -> String {
    format!("{source_id}:n:{}", machine_key(info))
}

fn container_id(source_id: &str, c: &ContainerSummary) -> String {
    format!("{source_id}:c:{}", c.id.get(..12).unwrap_or(&c.id))
}

/// How a container is doing, from what the engine says. `None` is a container that finished by itself (exit code 0): not a problem, and not
/// worth a place on the map either.
pub fn container_state(c: &ContainerSummary) -> Option<(Own, String)> {
    match c.state.as_str() {
        "running" if c.status.contains("(unhealthy)") => Some((Own::Warn, "unhealthy".into())),
        "running" => Some((Own::Ok, String::new())),
        "paused" => Some((Own::Warn, "paused".into())),
        "created" => Some((Own::Warn, "created, never started".into())),
        "restarting" => Some((Own::Crit, "restarting".into())),
        "dead" => Some((Own::Crit, "dead".into())),
        "exited" => match exit_code(&c.status) {
            Some(0) => None,
            Some(n) => Some((Own::Crit, format!("exited with code {n}"))),
            None => Some((Own::Crit, "exited".into())),
        },
        _ => Some((Own::Warn, c.state.clone())),
    }
}

/// `Exited (137) 2 hours ago` is 137.
fn exit_code(status: &str) -> Option<i32> {
    let rest = status.strip_prefix("Exited (")?;
    rest[..rest.find(')')?].parse().ok()
}

/// The machine as a host under the source's cluster, and its containers as workloads on it. `location` is the admin's word for where the
/// machine is (a rack, a site), empty for none.
pub fn build(
    source_id: &str,
    info: &EngineInfo,
    location: &str,
    containers: &[ContainerSummary],
    networks: &[SwarmNetwork],
    started_ms: i64,
) -> (Vec<PbNode>, Vec<Edge>) {
    let host = host_id(source_id, info);
    let mut meta = json!({
        "role": "docker",
        "osType": if info.os_type.is_empty() { "linux" } else { info.os_type.as_str() },
        "arch": normalize_arch(&info.architecture),
        "vcpu": info.ncpu,
        "ram": info.mem_total as f64 / GIB,
        "os": format!("{} · Docker {}", info.operating_system, info.server_version),
    });
    if !location.is_empty() {
        meta["location"] = json!(location);
    }
    let mut out = vec![PbNode {
        id: host.clone(),
        kind: NodeKind::Host.into(),
        name: info.name.clone(),
        parent: Some(source_id.to_string()),
        provider: Provider::Docker.into(),
        own: Own::Ok.into(),
        since: started_ms,
        meta: Some(struct_from_json(meta)),
        ..Default::default()
    }];

    let mut sorted: Vec<&ContainerSummary> = containers.iter().collect();
    sorted.sort_by(|a, b| name_of(a).cmp(name_of(b)));
    for c in sorted {
        let Some((own, reason)) = container_state(c) else {
            continue;
        };
        let image = short_image(&c.image);
        let stack = c
            .labels
            .as_ref()
            .and_then(|l| l.get(COMPOSE_PROJECT))
            .filter(|s| !s.is_empty())
            .map_or("—", String::as_str);
        let container = json!({
            "name": c.id.get(..12).unwrap_or(&c.id), "image": image,
            "ready": c.state == "running", "restarts": 0, "state": c.state,
        });
        out.push(PbNode {
            id: container_id(source_id, c),
            kind: NodeKind::Workload.into(),
            name: name_of(c).to_string(),
            parent: Some(host.clone()),
            provider: Provider::Docker.into(),
            own: own.into(),
            reason,
            since: c.created * 1000,
            meta: Some(struct_from_json(json!({
                "type": "Container", "image": image, "ns": stack, "restarts": 0,
                "containers": [container], "phase": c.state,
            }))),
            ..Default::default()
        });
    }

    // ---- the networks you made, with who is on them
    let machine = machine_key(info);
    let mut edges = Vec::new();
    for (n, members) in drawn_networks(containers, networks) {
        let id = network_id(source_id, &machine, n);
        let subnet = n
            .ipam
            .config
            .iter()
            .flatten()
            .map(|c| c.subnet.as_str())
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        let project = n
            .labels
            .as_ref()
            .and_then(|l| l.get(COMPOSE_PROJECT))
            .filter(|s| !s.is_empty())
            .map_or("—", String::as_str);
        out.push(PbNode {
            id: id.clone(),
            kind: NodeKind::Network.into(),
            name: n.name.clone(),
            parent: Some(source_id.to_string()),
            provider: Provider::Docker.into(),
            own: Own::Ok.into(),
            since: started_ms,
            meta: Some(struct_from_json(json!({
                "type": format!("Network ({})", n.driver), "netKind": n.driver, "subnet": subnet, "ns": project,
                "internal": n.internal, "members": members.len(), "machine": info.name,
            }))),
            ..Default::default()
        });
        for c in members {
            let to = container_id(source_id, c);
            edges.push(Edge {
                id: format!("{id}>{to}"),
                from: id.clone(),
                to,
                r#type: EdgeType::Route.into(),
                ..Default::default()
            });
        }
    }
    (out, edges)
}

fn network_id(source_id: &str, machine: &str, n: &SwarmNetwork) -> String {
    format!(
        "{source_id}:net:{machine}:{}",
        n.id.get(..12).unwrap_or(&n.id)
    )
}

/// The networks worth a place on the map, each with the containers on it that are on the map too. `bridge`, `host` and `none` are the
/// engine's own plumbing and every container is on one of them, which says nothing; a network nobody is on is not drawn either.
fn drawn_networks<'a>(
    containers: &'a [ContainerSummary],
    networks: &'a [SwarmNetwork],
) -> Vec<(&'a SwarmNetwork, Vec<&'a ContainerSummary>)> {
    let shown: Vec<&ContainerSummary> = containers
        .iter()
        .filter(|c| container_state(c).is_some())
        .collect();
    let mut sorted: Vec<&SwarmNetwork> = networks
        .iter()
        .filter(|n| {
            !n.ingress
                && !matches!(n.name.as_str(), "bridge" | "host" | "none")
                && n.driver != "null"
        })
        .collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    sorted
        .into_iter()
        .filter_map(|n| {
            let mut members: Vec<&ContainerSummary> = shown
                .iter()
                .copied()
                .filter(|c| attached(c).any(|(_, net)| net.id == n.id))
                .collect();
            members.sort_by(|a, b| name_of(a).cmp(name_of(b)));
            (!members.is_empty()).then_some((n, members))
        })
        .collect()
}

fn attached(
    c: &ContainerSummary,
) -> impl Iterator<Item = (&String, &crate::docker::ContainerNetwork)> {
    c.network_settings
        .iter()
        .filter_map(|s| s.networks.as_ref())
        .flatten()
}

/// The container -> link to its network, for the containers whose traffic can be told to belong to one: a container on exactly one network
/// has one interface, and everything it moves went over that network. On several the interfaces do not say which is which, and no rate is
/// claimed.
pub fn single_network_links(
    source_id: &str,
    info: &EngineInfo,
    containers: &[ContainerSummary],
    networks: &[SwarmNetwork],
) -> HashMap<String, String> {
    let machine = machine_key(info);
    let mut out = HashMap::new();
    for (n, members) in drawn_networks(containers, networks) {
        for c in members {
            if attached(c).count() == 1 {
                let (net, to) = (
                    network_id(source_id, &machine, n),
                    container_id(source_id, c),
                );
                out.insert(to.clone(), format!("{net}>{to}"));
            }
        }
    }
    out
}

fn name_of(c: &ContainerSummary) -> &str {
    c.names
        .first()
        .map_or(c.id.get(..12).unwrap_or(&c.id), |n| {
            n.trim_start_matches('/')
        })
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis()),
    )
    .unwrap_or(0)
}

/// Wires up a Docker agent's `main`, like [`crate::run_agent`] does for a swarm. The host node is named after the engine, which the
/// install manifest cannot know, so it is asked of the engine here before the first hello.
pub async fn run_agent(
    version: &str,
    default_socket: &str,
    new_sampler: impl Fn() -> Box<dyn ISampler> + Send + Sync + 'static,
) -> Result<()> {
    let mut cfg = Config::from_env("docker", version)?;
    let socket = std::env::var("DOCKER_SOCKET")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_socket.to_string());
    let location = std::env::var("NODE_LOCATION").unwrap_or_default();
    let location = location.trim().to_string();
    if cfg.host.is_empty() {
        let engine = Engine::new(connector_for(&socket));
        let mut last = None;
        for _ in 0..10 {
            match engine.get::<EngineInfo>("/info").await {
                Ok(info) => {
                    cfg.host = host_id(&cfg.source_id, &info);
                    last = None;
                    break;
                }
                Err(e) => {
                    last = Some(e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        if let Some(e) = last {
            return Err(e).context("cannot reach the Docker engine");
        }
    }
    let source_id = cfg.source_id.clone();
    hermes_agentkit::run_with(cfg, None, move |sink: SharedSink| {
        let source_id = source_id.clone();
        let location = location.clone();
        let engine = Arc::new(Engine::new(connector_for(&socket)));
        let sampler = new_sampler();
        async move { run(&source_id, &location, engine, sampler, sink).await }
    })
    .await
}

/// Reports this machine and its containers until the collector fails (the caller restarts it).
pub async fn run(
    source_id: &str,
    location: &str,
    engine: Arc<Engine>,
    mut sampler: Box<dyn ISampler>,
    sink: SharedSink,
) -> Result<()> {
    let info: EngineInfo = engine
        .get("/info")
        .await
        .context("cannot reach the Docker engine")?;
    let host = host_id(source_id, &info);
    let ncpu = info.ncpu.max(1) as f64;
    let mut mem_total = info.mem_total as f64 / MIB;
    let windows = info.os_type == "windows";
    let started = now_ms();
    let mut net = Tracker::new();
    let mut sent: (Vec<PbNode>, Vec<Edge>) = (Vec::new(), Vec::new());
    let mut last_info = String::new();

    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let containers: Vec<ContainerSummary> = engine
            .get("/containers/json?all=1")
            .await
            .context("listing containers")?;
        // a machine with no network of its own to show is still a machine: the list is only for the map
        let networks: Vec<SwarmNetwork> = engine.get("/networks").await.unwrap_or_default();
        let built = build(source_id, &info, location, &containers, &networks, started);
        if built != sent {
            sink.contribute_with_edges(built.0.clone(), built.1.clone());
            sent = built;
        }
        let single = single_network_links(source_id, &info, &containers, &networks);
        let workloads = sent
            .0
            .iter()
            .filter(|n| n.kind() == NodeKind::Workload)
            .count();
        let line = format!("{} · {workloads} containers", info.name);
        if line != last_info {
            sink.report(CollectorState::Connected, &line);
            last_info = line;
        }

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
                out.insert(host.clone(), m);
            }
        }

        let running: Vec<&ContainerSummary> =
            containers.iter().filter(|c| c.state == "running").collect();
        let (measured, links) = measure(
            &engine,
            source_id,
            &running,
            &single,
            Scale {
                ncpu,
                mem_total_mib: mem_total,
                windows,
            },
            &mut net,
        )
        .await;
        sink.alive(measured.keys().cloned().collect());
        out.extend(measured);
        if !out.is_empty() || !links.is_empty() {
            sink.metrics(out, links);
        }
    }
}

/// CPU, memory and traffic of the running containers, a few at a time.
/// What a container's numbers are measured against: this machine's cores and memory.
#[derive(Clone, Copy)]
struct Scale {
    ncpu: f64,
    mem_total_mib: f64,
    windows: bool,
}

async fn measure(
    engine: &Arc<Engine>,
    source_id: &str,
    running: &[&ContainerSummary],
    single: &HashMap<String, String>,
    scale: Scale,
    net: &mut Tracker,
) -> (HashMap<String, HashMap<String, f64>>, HashMap<String, f64>) {
    let Scale {
        ncpu,
        mem_total_mib,
        windows,
    } = scale;
    let permits = Arc::new(Semaphore::new(PARALLEL));
    let mut jobs = JoinSet::new();
    let mut live = HashSet::new();
    for c in running {
        let (id, key) = (c.id.clone(), container_id(source_id, c));
        live.insert(key.clone());
        let (engine, permits) = (engine.clone(), permits.clone());
        jobs.spawn(async move {
            let _permit = permits.acquire_owned().await.ok()?;
            let stats = tokio::time::timeout(
                Duration::from_secs(6),
                engine.get::<ContainerStats>(&format!("/containers/{id}/stats?stream=false")),
            )
            .await
            .ok()?
            .map_err(|e| warn!("docker: stats of {id}: {e:#}"))
            .ok()?;
            Some((key, stats))
        });
    }
    let mut out = HashMap::new();
    let mut links = HashMap::new(); // network -> container: what it moved on that network, Mbit/s
    while let Some(done) = jobs.join_next().await {
        let Ok(Some((key, s))) = done else { continue };
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
            let (rx, tx) = s
                .networks
                .values()
                .fold((0, 0), |(rx, tx), n| (rx + n.rx_bytes, tx + n.tx_bytes));
            let at_ms = parse_ms(&s.read).unwrap_or_else(now_ms);
            if let Some((r, t)) = net.update(&key, Sample { rx, tx, at_ms }) {
                m.insert("rxMbps".to_string(), r);
                m.insert("txMbps".to_string(), t);
                if let Some(link) = single.get(&key) {
                    links.insert(link.clone(), r + t);
                }
            }
        }
        out.insert(key, m);
    }
    net.keep(&live);
    (out, links)
}

#[cfg(test)]
#[path = "../tests/unit/standalone.rs"]
mod tests;
