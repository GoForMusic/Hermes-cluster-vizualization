//! Accepts what agents report, applies it to the live state and watches for silent agents (an agent that stops reporting is a source
//! that went down). `IIngestService` is the interface `controller` and `grpc` depend on; `IngestServiceImp` is its implementation.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hermes_proto::v1 as pb;
use hermes_proto::value::json_from_struct;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

use crate::database::ISourceDAO;
use crate::model::{Edge, Meta, Node, Source, own_name};
use crate::services::IGuard;
use crate::services::IStore;

const SILENT_AFTER: Duration = Duration::from_secs(20);
/// How long, after the hub starts, a source that used to be connected gets to report before it is declared down: the hub keeps its
/// topology in memory, so an agent that is gone for good would otherwise leave nothing to notice.
const NO_AGENT_AFTER: Duration = Duration::from_secs(30);
/// An agent that has not been heard of for this long is gone for good (its pod or container was replaced).
const FORGET_AFTER: Duration = Duration::from_secs(3600);
/// A Docker source is one machine. Another machine may take its place once the first has said nothing for this long: it was replaced.
const REPLACED_AFTER: Duration = Duration::from_secs(300);

/// One thing an agent reports.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The whole topology of the source.
    Snapshot {
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    },
    Status {
        id: String,
        own: String,
        reason: String,
    },
    Meta {
        id: String,
        patch: Meta,
    },
    Metrics {
        nodes: HashMap<String, HashMap<String, f64>>,
        edges: HashMap<String, f64>,
    },
    /// The collector lost or regained its source (the agent itself is alive).
    Report {
        state: String,
        info: String,
    },
    /// The workload nodes running on the agent's own host.
    Alive(Vec<String>),
    /// Nodes only this agent can see (the volumes of its own machine), added to the topology of the source.
    Contribution(Vec<Node>, Vec<Edge>),
    /// Who talked to whom on the agent's host (from its connection tracking).
    Flows(Vec<pb::Flow>),
}

impl Event {
    pub fn from_proto(event: pb::Event) -> Option<Self> {
        use pb::event::Kind;
        Some(match event.kind? {
            Kind::Snapshot(s) => Event::Snapshot {
                nodes: s.nodes.into_iter().map(Node::from).collect(),
                edges: s.edges.into_iter().map(Edge::from).collect(),
            },
            Kind::Status(s) => Event::Status {
                own: own_name(s.own()).to_string(),
                reason: s.reason,
                id: s.id,
            },
            Kind::Meta(m) => Event::Meta {
                patch: m
                    .patch
                    .as_ref()
                    .map(json_from_struct)
                    .and_then(into_object)
                    .unwrap_or_default(),
                id: m.id,
            },
            Kind::Metrics(m) => Event::Metrics {
                nodes: m.nodes.into_iter().map(|(id, v)| (id, v.values)).collect(),
                edges: m.edges,
            },
            Kind::Report(r) => Event::Report {
                state: match r.state() {
                    pb::CollectorState::Connected => "connected",
                    pb::CollectorState::Error => "error",
                    pb::CollectorState::Unspecified => "",
                }
                .to_string(),
                info: r.info,
            },
            Kind::Alive(a) => Event::Alive(a.ids),
            Kind::Contribution(c) => Event::Contribution(
                c.nodes.into_iter().map(Node::from).collect(),
                c.edges.into_iter().map(Edge::from).collect(),
            ),
            Kind::Flows(f) => Event::Flows(f.flows),
        })
    }
}

fn into_object(v: Value) -> Option<Meta> {
    match v {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// One running instance of an agent. A source can have several (Swarm runs one on every node); only some of them describe the cluster
/// (the topology), the rest just report their own machine's numbers.
struct Agent {
    seen: Instant,
    /// The host node it runs on, when it says.
    host: String,
    /// Workload nodes it says are running on its host.
    alive: HashSet<String>,
    /// Has sent topology (snapshot, status or meta events).
    topo: bool,
    /// What it said about itself in its hello.
    version: String,
    collector: String,
    protocol: u32,
}

impl Agent {
    fn new(now: Instant) -> Self {
        Self {
            seen: now,
            host: String::new(),
            alive: HashSet::new(),
            topo: false,
            version: String::new(),
            collector: String::new(),
            protocol: 0,
        }
    }
}

/// What an agent says about itself, and when it was last heard. Internal bookkeeping only — never serialized; `controller` converts
/// this into the wire-facing `model::AgentView` at the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub id: String,
    pub version: String,
    pub collector: String,
    pub host: String,
    pub seen_ago: Duration,
    pub protocol: u32,
}

#[derive(Default)]
struct State {
    /// source id -> agent id -> liveness
    agents: HashMap<String, HashMap<String, Agent>>,
    state: HashMap<String, String>,
    /// Sources marked down because their agents stopped reporting (not because one said so).
    silent: HashSet<String>,
    /// source id -> workloads a live agent vouched for at the last check (to tell when it stops)
    vouched: HashMap<String, HashSet<String>>,
    /// source id -> reporter (host or agent) -> when, and what it saw of who talked to whom
    flows: HashMap<String, HashMap<String, (Instant, Vec<pb::Flow>)>>,
    /// source id -> the links that carried a rate at the last look (to zero the ones that stop)
    flow_edges: HashMap<String, HashSet<String>>,
}

/// A report of flows older than this is not taken into account: its agent is gone or stuck.
const FLOWS_TTL: Duration = Duration::from_secs(20);
/// How many of the busiest connections are sent to the wallboard.
const TOP_FLOWS: usize = 8;

/// What an agent says about itself in its hello, besides who it is and where it runs.
#[derive(Debug, Clone, Copy)]
pub struct HelloInfo<'a> {
    pub version: &'a str,
    pub collector: &'a str,
    /// The wire protocol it speaks (`Hello.protocol`), compared against `hermes_proto::PROTOCOL` to warn when it is behind.
    pub protocol: u32,
}

/// What `controller` and `grpc` depend on: nothing here mentions liveness bookkeeping, flow TTLs or agent tracking.
pub trait IIngestService: Send + Sync {
    /// Finds the agent source that owns this token.
    fn authenticate(&self, token: &str) -> Option<Source>;
    /// An agent opened its stream and said who it is. It counts as heard from, and what it said about itself is kept for the admin.
    fn hello(&self, src: &Source, agent: &str, host: &str, info: HelloInfo<'_>);
    /// May this agent report to the source? A Docker source is one machine: an agent of another machine is turned away while the first
    /// one is still around (the reason is for the agent's log). Every other kind of source takes any agent that has its token.
    fn admit(&self, src: &Source, agent: &str, host: &str) -> Result<(), String>;
    /// The agents of a source, by id, with what each of them said about itself.
    fn agents(&self, source_id: &str) -> Vec<AgentInfo>;
    /// Applies one batch from an agent and says whether the agent must send its snapshot again (the hub does not have it).
    fn handle(&self, src: &Source, agent: &str, host: &str, events: Vec<Event>) -> bool;
    /// Looks at every agent source and marks the ones whose agents went quiet. Meant to be called on a timer
    /// (`app::spawn_background`) — kept synchronous here (unlike the old `watch(self: Arc<Self>)`) so the trait
    /// stays object-safe; the ticking loop itself lives in `app.rs`, same as `IEngine::evaluate`.
    fn check(&self, now: Instant);
}

pub struct IngestServiceImp {
    guard: Arc<dyn IGuard>,
    store: Arc<dyn IStore>,
    db: Arc<dyn ISourceDAO>,
    started: Instant,
    state: Mutex<State>,
}

impl IngestServiceImp {
    pub fn new(guard: Arc<dyn IGuard>, store: Arc<dyn IStore>, db: Arc<dyn ISourceDAO>) -> Self {
        Self::started_at(guard, store, db, Instant::now())
    }

    pub fn started_at(
        guard: Arc<dyn IGuard>,
        store: Arc<dyn IStore>,
        db: Arc<dyn ISourceDAO>,
        started: Instant,
    ) -> Self {
        Self {
            guard,
            store,
            db,
            started,
            state: Mutex::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_state(&self, id: &str, state: &str, info: &str) {
        let changed = self
            .lock()
            .state
            .insert(id.to_string(), state.to_string())
            .as_deref()
            != Some(state);
        if changed {
            self.store.set_stale(id, state == "error");
            let _ = self.db.set_source_state(id, state, info);
            self.store.publish(&json!({"type": "sources"}));
        }
    }

    fn hello_at(&self, now: Instant, src: &Source, agent: &str, host: &str, info: HelloInfo<'_>) {
        let changed = {
            let mut guard = self.lock();
            let ag = guard
                .agents
                .entry(src.id.clone())
                .or_default()
                .entry(agent.to_string())
                .or_insert_with(|| Agent::new(now));
            ag.seen = now;
            let host_changed = !host.is_empty() && ag.host != host;
            let changed = host_changed
                || ag.version != info.version
                || ag.collector != info.collector
                || ag.protocol != info.protocol;
            if host_changed {
                ag.host = host.to_string();
            }
            ag.version = info.version.to_string();
            ag.collector = info.collector.to_string();
            ag.protocol = info.protocol;
            changed
        };
        // A new or upgraded agent should show up in the admin without a manual refresh.
        if changed {
            self.store.publish(&json!({"type": "sources"}));
        }
    }

    fn admit_at(&self, now: Instant, src: &Source, agent: &str, host: &str) -> Result<(), String> {
        if src.provider() != "docker" || host.is_empty() {
            return Ok(());
        }
        let guard = self.lock();
        let other = guard
            .agents
            .get(&src.id)
            .into_iter()
            .flatten()
            .find(|(id, a)| {
                id.as_str() != agent
                    && !a.host.is_empty()
                    && a.host != host
                    && now.saturating_duration_since(a.seen) <= REPLACED_AFTER
            });
        match other {
            Some((_, a)) => Err(format!(
                "this source is one Docker machine and already has {}: add a source for this machine too",
                a.host.rsplit(":n:").next().unwrap_or(&a.host)
            )),
            None => Ok(()),
        }
    }

    fn agents_at(&self, now: Instant, source_id: &str) -> Vec<AgentInfo> {
        let guard = self.lock();
        let mut list: Vec<AgentInfo> = guard
            .agents
            .get(source_id)
            .into_iter()
            .flatten()
            .map(|(id, a)| AgentInfo {
                id: id.clone(),
                version: a.version.clone(),
                collector: a.collector.clone(),
                host: a.host.clone(),
                seen_ago: now.saturating_duration_since(a.seen),
                protocol: a.protocol,
            })
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    fn handle_at(
        &self,
        now: Instant,
        src: &Source,
        agent: &str,
        host: &str,
        events: Vec<Event>,
    ) -> bool {
        // An agent whose source was removed keeps its stream until the next check of its token: what it sends in the meantime must not
        // draw the cluster again (nothing would ever remove it). Only what creates a topology needs the check.
        let creates = events
            .iter()
            .any(|e| matches!(e, Event::Snapshot { .. } | Event::Contribution(..)));
        if creates
            && !self
                .db
                .list_sources()
                .is_ok_and(|list| list.iter().any(|s| s.id == src.id))
        {
            return false;
        }
        // does this batch describe the cluster, or only this agent's machine?
        let topo_batch = events.iter().any(|e| {
            matches!(
                e,
                Event::Snapshot { .. } | Event::Status { .. } | Event::Meta { .. }
            )
        });
        let (accept_report, was_silent) = self.update_liveness(now, src, agent, host, topo_batch);
        let saw_snapshot = self.apply_events(now, src, agent, host, events, accept_report);

        let mut resync = !saw_snapshot && !self.store.has_source(&src.id);
        if was_silent && !saw_snapshot {
            // The agent is back but was unreachable for a while (hung, or cut off from the hub). What the hub holds is from before: ask
            // for a fresh snapshot, and stop showing the source as down.
            self.set_state(&src.id, "connected", "agent reporting again");
            resync = true;
        }
        resync
    }

    /// Records that this agent instance is still alive and decides two things from that alone: whether *its* view of the source's
    /// state counts (`accept_report`), and whether the source is coming back from being silent (`was_silent`).
    fn update_liveness(
        &self,
        now: Instant,
        src: &Source,
        agent: &str,
        host: &str,
        topo_batch: bool,
    ) -> (bool, bool) {
        let mut guard = self.lock();
        let st = &mut *guard;
        let agents = st.agents.entry(src.id.clone()).or_default();
        let ag = agents
            .entry(agent.to_string())
            .or_insert_with(|| Agent::new(now));
        ag.seen = now;
        if !host.is_empty() {
            ag.host = host.to_string();
        }
        ag.topo |= topo_batch;
        let reports_topology = ag.topo;
        // A collector's own state (cannot reach the API) is the cluster's state when it is the one describing the cluster, or when
        // nobody has described it yet (an agent that started while the API was down). A worker agent's state is only its own.
        let any_topo = agents.values().any(|a| a.topo);
        // the source is up again once an agent that describes the cluster is back (a worker's numbers say nothing about the cluster)
        (
            reports_topology || !any_topo,
            reports_topology && st.silent.remove(&src.id),
        )
    }

    /// Applies one batch's events to the live topology and state. Returns whether a snapshot was among them.
    fn apply_events(
        &self,
        now: Instant,
        src: &Source,
        agent: &str,
        host: &str,
        events: Vec<Event>,
        accept_report: bool,
    ) -> bool {
        let mut saw_snapshot = false;
        for event in events {
            match event {
                Event::Snapshot { nodes, edges } => {
                    saw_snapshot = true;
                    let count = nodes.len();
                    if self.guard.set_topology(&src.id, nodes, edges).is_err() {
                        // the guard wrote the state; remember it so that a later success is written too
                        self.lock().state.insert(src.id.clone(), "duplicate".into());
                        continue;
                    }
                    self.set_state(
                        &src.id,
                        "connected",
                        &format!("agent reporting · {count} nodes"),
                    );
                }
                Event::Contribution(nodes, edges) => {
                    // one contribution per host: a new instance of the agent of the same node replaces the old one
                    let key = if host.is_empty() { agent } else { host };
                    if src.provider() == "docker" {
                        // No agent describes this source as a whole: each machine adds itself. The cluster they hang from is the hub's.
                        self.store.ensure_placeholder(src);
                        self.store.set_stale(&src.id, false);
                        // and it is one machine: a machine that replaced the one before it leaves nothing of it behind
                        self.store.keep_only_contribution(&src.id, key);
                    }
                    self.store
                        .set_contribution_with_edges(&src.id, key, nodes, edges);
                }
                Event::Alive(ids) => {
                    if let Some(ag) = self
                        .lock()
                        .agents
                        .get_mut(&src.id)
                        .and_then(|a| a.get_mut(agent))
                    {
                        ag.alive = ids.into_iter().collect();
                    }
                }
                Event::Report { state, info } => {
                    if accept_report && (state == "error" || state == "connected") {
                        self.set_state(&src.id, &state, &info);
                    }
                }
                Event::Status { id, own, reason } => self.store.set_status(&id, &own, &reason),
                Event::Meta { id, patch } => self.store.set_meta(&id, patch),
                Event::Metrics { nodes, edges } => self.store.apply_metrics(&nodes, &edges),
                Event::Flows(flows) => {
                    let key = if host.is_empty() { agent } else { host };
                    self.lock()
                        .flows
                        .entry(src.id.clone())
                        .or_default()
                        .insert(key.to_string(), (now, flows));
                    self.recompute_flows(now, &src.id);
                }
            }
        }
        saw_snapshot
    }

    /// Looks at every agent source. What counts is the freshest agent that describes the cluster: on Swarm, worker agents keep reporting
    /// their own numbers while the manager, the only one that sees services and tasks, may be down; the source is down then. A source
    /// none of whose agents has sent topology yet is judged by all of them.
    /// Puts the rates the agents saw on the links of the source (and zeroes the ones that went quiet, or whose agent stopped reporting).
    fn recompute_flows(&self, now: Instant, source: &str) {
        let reports: Vec<Vec<pb::Flow>> = {
            let mut guard = self.lock();
            let Some(all) = guard.flows.get_mut(source) else {
                return;
            };
            all.retain(|_, (at, _)| now.saturating_duration_since(*at) < FLOWS_TTL);
            all.values().map(|(_, f)| f.clone()).collect()
        };
        // Only this source's own topology, not every source's cloned just to filter the rest back out.
        let mine = |id: &str| id == source || id.starts_with(&format!("{source}:"));
        let nodes: Vec<_> = self
            .store
            .nodes_for(source)
            .into_iter()
            .filter(|n| mine(&n.id))
            .collect();
        let edges: Vec<_> = self
            .store
            .edges_for(source)
            .into_iter()
            .filter(|e| mine(&e.id))
            .collect();
        let refs: Vec<&[pb::Flow]> = reports.iter().map(Vec::as_slice).collect();
        let mut rates = crate::flows::edge_rates(&nodes, &edges, &refs);
        let now_ids: HashSet<String> = rates.keys().cloned().collect();
        let before = self
            .lock()
            .flow_edges
            .insert(source.to_string(), now_ids.clone());
        for gone in before
            .into_iter()
            .flatten()
            .filter(|id| !now_ids.contains(id))
        {
            rates.insert(gone, 0.0);
        }
        if !rates.is_empty() {
            self.store.apply_metrics(&HashMap::new(), &rates);
        }
        let lines = crate::flows::top_flows(&nodes, &refs, TOP_FLOWS);
        self.store
            .publish(&json!({"type": "flows", "source": source, "flows": lines}));
    }

    fn judge(&self, now: Instant, src: &Source) -> Verdict {
        let mut guard = self.lock();
        let st = &mut *guard;
        let agents = st.agents.entry(src.id.clone()).or_default();
        agents.retain(|_, ag| now.saturating_duration_since(ag.seen) <= FORGET_AFTER);

        let any_seen = agents.values().map(|a| a.seen).max();
        let topo_seen = agents.values().filter(|a| a.topo).map(|a| a.seen).max();
        let have_topo = topo_seen.is_some();
        let seen = topo_seen.or(any_seen).unwrap_or(now);
        let silent = (topo_seen.or(any_seen)).is_some()
            && now.saturating_duration_since(seen) > SILENT_AFTER;
        // known before, but nothing has described the cluster since the hub started (workers may still be talking: they only report numbers)
        // Docker machines of their own have nobody that describes the whole: each reports itself (a contribution), so an agent that is
        // heard from is all there is to hear.
        let machines_report = src.provider() == "docker" && any_seen.is_some();
        let gone = !have_topo
            && !machines_report
            && src.state != "pending"
            && now.saturating_duration_since(self.started) > NO_AGENT_AFTER;
        if silent || gone {
            st.silent.insert(src.id.clone());
        }

        let mut host_live: HashMap<String, bool> = HashMap::new(); // hosts that run their own agent: alive while that agent's heartbeat is fresh
        let mut running: HashMap<&String, bool> = HashMap::new(); // workloads a live agent sees running on its host (used only while the source is down)
        for ag in agents.values() {
            let fresh = now.saturating_duration_since(ag.seen) <= SILENT_AFTER;
            if !ag.host.is_empty() {
                *host_live.entry(ag.host.clone()).or_default() |= fresh;
            }
            for id in &ag.alive {
                *running.entry(id).or_default() |= fresh;
            }
        }
        let down = src.state == "error" || silent || gone;
        let vouched_now: HashSet<String> = if down {
            running
                .into_iter()
                .filter(|(_, live)| *live)
                .map(|(id, _)| id.clone())
                .collect()
        } else {
            HashSet::new()
        };
        // nothing is being vouched for while the source is healthy
        let vouched_before = if down {
            st.vouched.insert(src.id.clone(), vouched_now.clone())
        } else {
            st.vouched.remove(&src.id)
        }
        .unwrap_or_default();
        Verdict {
            silent,
            gone,
            down,
            have_topo,
            seen,
            host_live,
            vouched_now,
            vouched_before,
        }
    }
}

impl IIngestService for IngestServiceImp {
    fn authenticate(&self, token: &str) -> Option<Source> {
        if token.is_empty() {
            return None;
        }
        self.db
            .list_sources()
            .ok()?
            .into_iter()
            .find(|s| s.is_agent() && bool::from(s.secret.as_bytes().ct_eq(token.as_bytes())))
    }

    fn hello(&self, src: &Source, agent: &str, host: &str, info: HelloInfo<'_>) {
        self.hello_at(Instant::now(), src, agent, host, info);
    }

    fn admit(&self, src: &Source, agent: &str, host: &str) -> Result<(), String> {
        self.admit_at(Instant::now(), src, agent, host)
    }

    fn agents(&self, source_id: &str) -> Vec<AgentInfo> {
        self.agents_at(Instant::now(), source_id)
    }

    fn handle(&self, src: &Source, agent: &str, host: &str, events: Vec<Event>) -> bool {
        self.handle_at(Instant::now(), src, agent, host, events)
    }

    fn check(&self, now: Instant) {
        let sources: Vec<String> = self.lock().flows.keys().cloned().collect();
        for source in sources {
            self.recompute_flows(now, &source);
        }
        let Ok(list) = self.db.list_sources() else {
            return;
        };
        for src in list.iter().filter(|s| s.is_agent()) {
            let verdict = self.judge(now, src);

            if verdict.gone {
                self.set_state(
                    &src.id,
                    "error",
                    "no agent has reported since the hub started",
                );
            }
            if verdict.silent || verdict.gone {
                self.store.ensure_placeholder(src); // does nothing when there is a topology to mark stale instead
            }
            if verdict.silent {
                let what = if verdict.have_topo {
                    "the agent that reports the cluster"
                } else {
                    "agent"
                };
                self.set_state(
                    &src.id,
                    "error",
                    &format!(
                        "{what} silent for {}",
                        fmt_duration(now.saturating_duration_since(verdict.seen))
                    ),
                );
            }
            // A host's own agent is better evidence about that host than the cluster's control plane: with the manager or the API down
            // the host is still up while its agent talks, and with everything else fine a host whose agent went quiet is not.
            // With the control plane unreachable the hosts' own view is all there is: what runs on a live host is running, what its agent
            // stopped vouching for is unknown again. With the source healthy the control plane is the authority, so nothing to do.
            // One batch per source: a control plane going down can flip many nodes at once, and every browser only needs one snapshot.
            let vouched_gone: Box<dyn Iterator<Item = &str>> = if verdict.down {
                Box::new(
                    verdict
                        .vouched_before
                        .difference(&verdict.vouched_now)
                        .map(String::as_str),
                )
            } else {
                Box::new(std::iter::empty())
            };
            // `IStore::set_nodes_stale` takes a slice, not an iterator, so it stays object-safe (a generic method could not be
            // called through `Arc<dyn IStore>`) — collecting first costs nothing that matters here (once per source per tick).
            let changes: Vec<(&str, bool)> = verdict
                .host_live
                .iter()
                .map(|(host, live)| (host.as_str(), !live))
                .chain(verdict.vouched_now.iter().map(|id| (id.as_str(), false)))
                .chain(vouched_gone.map(|id| (id, true)))
                .collect();
            self.store.set_nodes_stale(&changes);
        }
    }
}

struct Verdict {
    silent: bool,
    gone: bool,
    down: bool,
    have_topo: bool,
    seen: Instant,
    host_live: HashMap<String, bool>,
    vouched_now: HashSet<String>,
    vouched_before: HashSet<String>,
}

/// `25s`, `1m5s`, `2h0m0s`
fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs() + u64::from(d.subsec_millis() >= 500);
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m{}s", s / 60, s % 60),
        _ => format!("{}h{}m{}s", s / 3600, s % 3600 / 60, s % 60),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ingest.rs"]
mod tests;
