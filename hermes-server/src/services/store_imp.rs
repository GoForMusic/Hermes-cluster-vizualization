//! The live topology: one topology per source, a merged view of all of them, and the fan-out of every change to the browsers.
//! `IStore` is the interface every other service and controller depends on; `StoreImp` is its implementation. Held on
//! `AppState` and shared everywhere, unlike the request-scoped DAOs — the one deliberate exception to "depend on the
//! narrow interface you need", since almost everything needs the live topology.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::database::now_ms;
use crate::model::{Edge, Meta, Node, Source};

/// Events a slow browser may fall behind by before it is dropped; its `EventSource` reconnects and gets a fresh snapshot.
const BACKLOG: usize = 64;

/// What one host contributed: nodes, and the links between them.
type Contributed = (Vec<Node>, Vec<Edge>);

struct Topo {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    stale: bool,
    /// What individual agents contributed on top of the topology (the volumes of their own machine), by host. A new topology from the
    /// agent that describes the cluster leaves them alone.
    extra: BTreeMap<String, Vec<Node>>,
    /// The links that came with those nodes, by host: replaced together with them.
    extra_edges: BTreeMap<String, Vec<Edge>>,
}

impl Topo {
    fn all_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().chain(self.extra_edges.values().flatten())
    }

    fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().chain(self.extra.values().flatten())
    }

    fn all_nodes_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.nodes
            .iter_mut()
            .chain(self.extra.values_mut().flatten())
    }
}

/// Where a node is kept: in the topology itself, or in what a host contributed.
#[derive(Clone)]
enum Slot {
    Base(usize),
    Extra(String, usize),
}

#[derive(Default)]
struct Inner {
    order: Vec<String>,
    topos: HashMap<String, Topo>,
    /// node / edge id -> (source, position in its topology). A later source wins when two of them use the same id.
    nodes: HashMap<String, (String, Slot)>,
    edges: HashMap<String, (String, Slot)>,
    /// Contributions that came before the topology they belong to: source -> host -> nodes and links.
    pending: HashMap<String, BTreeMap<String, Contributed>>,
}

impl Inner {
    fn reindex(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        for src in &self.order {
            let topo = &self.topos[src];
            for (i, n) in topo.nodes.iter().enumerate() {
                self.nodes
                    .insert(n.id.clone(), (src.clone(), Slot::Base(i)));
            }
            for (host, list) in &topo.extra {
                for (i, n) in list.iter().enumerate() {
                    self.nodes
                        .insert(n.id.clone(), (src.clone(), Slot::Extra(host.clone(), i)));
                }
            }
            for (i, e) in topo.edges.iter().enumerate() {
                self.edges
                    .insert(e.id.clone(), (src.clone(), Slot::Base(i)));
            }
            for (host, list) in &topo.extra_edges {
                for (i, e) in list.iter().enumerate() {
                    self.edges
                        .insert(e.id.clone(), (src.clone(), Slot::Extra(host.clone(), i)));
                }
            }
        }
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut Node> {
        let (src, slot) = self.nodes.get(id)?.clone();
        let topo = self.topos.get_mut(&src)?;
        match slot {
            Slot::Base(i) => topo.nodes.get_mut(i),
            Slot::Extra(host, i) => topo.extra.get_mut(&host)?.get_mut(i),
        }
    }

    fn edge_mut(&mut self, id: &str) -> Option<&mut Edge> {
        let (src, slot) = self.edges.get(id)?.clone();
        let topo = self.topos.get_mut(&src)?;
        match slot {
            Slot::Base(i) => topo.edges.get_mut(i),
            Slot::Extra(host, i) => topo.extra_edges.get_mut(&host)?.get_mut(i),
        }
    }

    fn snapshot_json(&self) -> String {
        #[derive(Serialize)]
        struct Snapshot<'a> {
            r#type: &'static str,
            nodes: Vec<&'a Node>,
            edges: Vec<&'a Edge>,
        }
        let topos = || self.order.iter().map(|s| &self.topos[s]);
        let snapshot = Snapshot {
            r#type: "snapshot",
            nodes: topos().flat_map(Topo::all_nodes).collect(),
            edges: topos().flat_map(Topo::all_edges).collect(),
        };
        serde_json::to_string(&snapshot).unwrap_or_default()
    }
}

/// What `services` and `controller` depend on: nothing here mentions the internal index, the pending-contribution
/// buffer or how a snapshot is broadcast.
pub trait IStore: Send + Sync {
    /// A stream of JSON events. Subscribe first, then read the snapshot: nothing is missed in between.
    fn subscribe(&self) -> broadcast::Receiver<Arc<str>>;
    /// Sends any event to every connected browser.
    fn publish(&self, event: &Value);
    /// Replaces everything a source contributes and tells all browsers to rebuild.
    fn set_topology(&self, src: &str, nodes: Vec<Node>, edges: Vec<Edge>);
    /// What one host contributes to a source's topology, for example the volumes only its own agent can measure. It replaces what the same
    /// host contributed before, and survives a new topology of the source. Before the source has a topology it waits for one.
    fn set_contribution(&self, src: &str, host: &str, nodes: Vec<Node>) {
        self.set_contribution_with_edges(src, host, nodes, Vec::new());
    }
    /// The same, with the links between the nodes (a Docker network and the containers on it). Both are replaced together.
    fn set_contribution_with_edges(
        &self,
        src: &str,
        host: &str,
        nodes: Vec<Node>,
        edges: Vec<Edge>,
    );
    /// The source was renamed: its cluster shows the new name at once.
    fn rename_cluster(&self, src: &str, name: &str);
    /// Drops what every other host contributed to the source: for a source that is one machine, which a new machine replaces.
    fn keep_only_contribution(&self, src: &str, host: &str);
    /// Marks everything a source reported as out of date (or current again). While a source cannot be reached the hub still holds its
    /// last state, and showing that as if it were live would hide the outage.
    fn set_stale(&self, src: &str, stale: bool);
    /// Makes a source that has reported nothing (yet, or since the hub restarted) show up as a cluster with no data, instead of not
    /// showing at all. A real snapshot replaces it. Does nothing when the source already has a topology.
    fn ensure_placeholder(&self, src: &Source);
    /// Marks one node out of date (or current), for a host whose own agent says something different from what the source as a whole
    /// does: alive while the control plane is unreachable, or silent while everything else is fine.
    fn set_node_stale(&self, id: &str, stale: bool);
    /// Same as `set_node_stale`, for many nodes at once: one lock and, if anything actually changed, one broadcast — not one per node.
    /// A control plane going down can flip a whole cluster's hosts `stale` in the same tick; every connected browser only needs to
    /// hear about that once.
    fn set_nodes_stale(&self, changes: &[(&str, bool)]);
    fn remove_source(&self, src: &str);
    /// Has the source published a topology? An agent that lost it must send it again.
    fn has_source(&self, src: &str) -> bool;
    fn snapshot_json(&self) -> String;
    /// Records the state a source reports for one node.
    fn set_status(&self, id: &str, own: &str, reason: &str);
    /// Merges fields into a node's meta (restarts, containers, ...) and broadcasts the patch.
    fn set_meta(&self, id: &str, patch: Meta);
    /// Merges node metrics and edge traffic, then broadcasts them.
    fn apply_metrics(
        &self,
        nodes: &HashMap<String, HashMap<String, f64>>,
        edges: &HashMap<String, f64>,
    );
    /// Copies that are safe to use without holding the lock.
    fn nodes(&self) -> Vec<Node>;
    fn edges(&self) -> Vec<Edge>;
    /// Copies of one source's own nodes, without cloning every other source's topology first just to filter it back out
    /// (`recompute_flows` calls this on every flows report, so it matters here more than at `nodes()`'s call sites).
    fn nodes_for(&self, src: &str) -> Vec<Node>;
    fn edges_for(&self, src: &str) -> Vec<Edge>;
}

/// Keeps one topology per source and a merged view of all of them.
pub struct StoreImp {
    inner: RwLock<Inner>,
    tx: broadcast::Sender<Arc<str>>,
}

impl Default for StoreImp {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreImp {
    pub fn new() -> Self {
        Self {
            inner: RwLock::default(),
            tx: broadcast::channel(BACKLOG).0,
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    // Broadcasting happens while the caller still holds the lock, so browsers see changes in the order they were made.
    fn send(&self, msg: impl Into<Arc<str>>) {
        let _ = self.tx.send(msg.into()); // an error only means that nobody is listening
    }
}

impl IStore for StoreImp {
    fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.tx.subscribe()
    }

    fn publish(&self, event: &Value) {
        let _guard = self.write();
        self.send(event.to_string());
    }

    fn set_topology(&self, src: &str, nodes: Vec<Node>, edges: Vec<Edge>) {
        let mut g = self.write();
        let (mut extra, mut extra_edges) = g
            .topos
            .remove(src)
            .map(|t| (t.extra, t.extra_edges))
            .unwrap_or_default();
        for (host, (list, links)) in g.pending.remove(src).unwrap_or_default() {
            extra.insert(host.clone(), list);
            extra_edges.insert(host, links);
        }
        if !g.order.iter().any(|o| o == src) {
            g.order.push(src.to_string());
        }
        g.topos.insert(
            src.to_string(),
            Topo {
                nodes,
                edges,
                stale: false,
                extra,
                extra_edges,
            },
        );
        g.reindex();
        self.send(g.snapshot_json());
    }

    fn set_contribution_with_edges(
        &self,
        src: &str,
        host: &str,
        mut nodes: Vec<Node>,
        edges: Vec<Edge>,
    ) {
        let mut g = self.write();
        let Some(stale) = g.topos.get(src).map(|t| t.stale) else {
            g.pending
                .entry(src.to_string())
                .or_default()
                .insert(host.to_string(), (nodes, edges));
            return;
        };
        nodes.iter_mut().for_each(|n| n.stale = stale);
        if let Some(topo) = g.topos.get_mut(src) {
            topo.extra.insert(host.to_string(), nodes);
            topo.extra_edges.insert(host.to_string(), edges);
        }
        g.reindex();
        self.send(g.snapshot_json());
    }

    fn rename_cluster(&self, src: &str, name: &str) {
        let mut g = self.write();
        let mut changed = false;
        if let Some(topo) = g.topos.get_mut(src) {
            for n in topo
                .nodes
                .iter_mut()
                .filter(|n| n.kind == "cluster" && n.id == src && n.name != name)
            {
                n.name = name.to_string();
                changed = true;
            }
        }
        if changed {
            self.send(g.snapshot_json());
        }
    }

    fn keep_only_contribution(&self, src: &str, host: &str) {
        let mut g = self.write();
        let Some(topo) = g.topos.get_mut(src) else {
            return;
        };
        let others = topo.extra.len() - usize::from(topo.extra.contains_key(host));
        if others == 0 {
            return;
        }
        topo.extra.retain(|h, _| h == host);
        topo.extra_edges.retain(|h, _| h == host);
        g.reindex();
        self.send(g.snapshot_json());
    }

    fn set_stale(&self, src: &str, stale: bool) {
        let mut g = self.write();
        let Some(topo) = g.topos.get_mut(src).filter(|t| t.stale != stale) else {
            return;
        };
        topo.stale = stale;
        topo.all_nodes_mut().for_each(|n| n.stale = stale);
        self.send(g.snapshot_json());
    }

    fn ensure_placeholder(&self, src: &Source) {
        let mut g = self.write();
        if g.topos.contains_key(&src.id) {
            return;
        }
        let meta: Meta = [
            ("version".to_string(), json!("—")),
            ("api".to_string(), json!("—")),
        ]
        .into_iter()
        .collect();
        let cluster = Node {
            id: src.id.clone(),
            kind: "cluster".into(),
            name: src.name.clone(),
            provider: src.provider().into(),
            own: "ok".into(),
            status: "ok".into(),
            since: now_ms(),
            meta,
            stale: true,
            ..Default::default()
        };
        g.order.push(src.id.clone());
        g.topos.insert(
            src.id.clone(),
            Topo {
                nodes: vec![cluster],
                edges: vec![],
                stale: true,
                extra: BTreeMap::new(),
                extra_edges: BTreeMap::new(),
            },
        );
        g.reindex();
        self.send(g.snapshot_json());
    }

    fn set_node_stale(&self, id: &str, stale: bool) {
        self.set_nodes_stale(&[(id, stale)]);
    }

    fn set_nodes_stale(&self, changes: &[(&str, bool)]) {
        let mut g = self.write();
        let mut changed = false;
        for &(id, stale) in changes {
            if let Some(node) = g.node_mut(id).filter(|n| n.stale != stale) {
                node.stale = stale;
                changed = true;
            }
        }
        if changed {
            self.send(g.snapshot_json());
        }
    }

    fn remove_source(&self, src: &str) {
        let mut g = self.write();
        g.pending.remove(src);
        if g.topos.remove(src).is_none() {
            return;
        }
        g.order.retain(|o| o != src);
        g.reindex();
        self.send(g.snapshot_json());
    }

    fn has_source(&self, src: &str) -> bool {
        self.read().topos.contains_key(src)
    }

    fn snapshot_json(&self) -> String {
        self.read().snapshot_json()
    }

    fn set_status(&self, id: &str, own: &str, reason: &str) {
        let mut g = self.write();
        let Some(node) = g.node_mut(id) else { return };
        node.own = own.to_string();
        node.status = own.to_string();
        node.reason = reason.to_string();
        node.since = now_ms();
        self.send(json!({"type": "status", "id": id, "own": own, "reason": reason}).to_string());
    }

    fn set_meta(&self, id: &str, patch: Meta) {
        let mut g = self.write();
        let Some(node) = g.node_mut(id) else { return };
        node.meta.extend(patch.clone());
        self.send(json!({"type": "meta", "id": id, "meta": patch}).to_string());
    }

    fn apply_metrics(
        &self,
        nodes: &HashMap<String, HashMap<String, f64>>,
        edges: &HashMap<String, f64>,
    ) {
        let mut g = self.write();
        for (id, values) in nodes {
            if let Some(node) = g.node_mut(id) {
                node.m.extend(values.iter().map(|(k, v)| (k.clone(), *v)));
            }
        }
        for (id, mbps) in edges {
            if let Some(edge) = g.edge_mut(id) {
                edge.mbps = *mbps;
            }
        }
        self.send(json!({"type": "metrics", "nodes": nodes, "edges": edges}).to_string());
    }

    fn nodes(&self) -> Vec<Node> {
        let g = self.read();
        g.order
            .iter()
            .flat_map(|s| g.topos[s].all_nodes().cloned())
            .collect()
    }

    fn edges(&self) -> Vec<Edge> {
        let g = self.read();
        g.order
            .iter()
            .flat_map(|s| g.topos[s].all_edges().cloned())
            .collect()
    }

    fn nodes_for(&self, src: &str) -> Vec<Node> {
        self.read()
            .topos
            .get(src)
            .map(|t| t.all_nodes().cloned().collect())
            .unwrap_or_default()
    }

    fn edges_for(&self, src: &str) -> Vec<Edge> {
        self.read()
            .topos
            .get(src)
            .map(|t| t.all_edges().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/live.rs"]
mod tests;
