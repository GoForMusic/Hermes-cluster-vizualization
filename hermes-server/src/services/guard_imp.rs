//! Stops the same cluster from being shown twice. A cluster is identified by the UID its collector reports (for Kubernetes: the UID of
//! the `kube-system` namespace), not by the name someone typed. The source that was added first keeps the cluster; a later one is
//! marked "duplicate". `IGuard` is the interface `IngestServiceImp` depends on; `GuardImp` is its implementation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;
use tracing::warn;

use crate::database::ISourceDAO;
use crate::model::{Edge, Node};
use crate::services::IStore;

/// Another source already shows this cluster.
#[derive(Debug, PartialEq, Eq)]
pub struct Duplicate;

/// What `IngestServiceImp` depends on: nothing here mentions the ownership map or how a "duplicate" message is deduplicated.
pub trait IGuard: Send + Sync {
    /// Applies a source's topology unless another source already shows the same cluster.
    fn set_topology(&self, src: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> Result<(), Duplicate>;
    /// Frees the cluster so another source can show it.
    fn remove_source(&self, src: &str);
}

pub struct GuardImp {
    store: Arc<dyn IStore>,
    db: Arc<dyn ISourceDAO>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// cluster uid -> the source that shows it
    owner: HashMap<String, String>,
    /// source id -> the last "duplicate" message written, to avoid rewriting it every second
    reported: HashMap<String, String>,
}

fn cluster_uid(nodes: &[Node]) -> Option<&str> {
    nodes
        .iter()
        .find(|n| n.kind == "cluster")
        .and_then(|n| n.meta.get("uid"))
        .and_then(|v| v.as_str())
        .filter(|u| !u.is_empty())
}

impl GuardImp {
    pub fn new(store: Arc<dyn IStore>, db: Arc<dyn ISourceDAO>) -> Self {
        Self {
            store,
            db,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The creation order of the sources, and their names.
    fn order(&self) -> (HashMap<String, usize>, HashMap<String, String>) {
        let mut index = HashMap::new();
        let mut names = HashMap::new();
        for (i, s) in self
            .db
            .list_sources()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            index.insert(s.id.clone(), i);
            names.insert(s.id, s.name);
        }
        (index, names)
    }

    fn mark(&self, id: &str, info: &str) {
        if self
            .state()
            .reported
            .insert(id.to_string(), info.to_string())
            .as_deref()
            == Some(info)
        {
            return;
        }
        if let Err(e) = self.db.set_source_state(id, "duplicate", info) {
            warn!("dedupe: {e:#}");
        }
        self.store.publish(&json!({"type": "sources"}));
    }
}

impl IGuard for GuardImp {
    fn set_topology(&self, src: &str, nodes: Vec<Node>, edges: Vec<Edge>) -> Result<(), Duplicate> {
        let Some(uid) = cluster_uid(&nodes).map(str::to_string) else {
            self.store.set_topology(src, nodes, edges); // the collector cannot identify its cluster: nothing to compare
            return Ok(());
        };
        let (index, names) = self.order();
        let name_of = |id: &str| names.get(id).cloned().unwrap_or_default();

        let mut st = self.state();
        match st.owner.get(&uid).cloned() {
            Some(current) if current != src => {
                let newcomer_first =
                    matches!((index.get(src), index.get(&current)), (Some(s), Some(c)) if s < c);
                if newcomer_first {
                    // the newcomer was added first: it takes the cluster over
                    st.owner.insert(uid, src.to_string());
                    st.reported.remove(src);
                    drop(st);
                    self.store.remove_source(&current);
                    self.mark(
                        &current,
                        &format!("this cluster is already added as “{}”", name_of(src)),
                    );
                    self.store.set_topology(src, nodes, edges);
                    Ok(())
                } else {
                    drop(st);
                    self.mark(
                        src,
                        &format!("this cluster is already added as “{}”", name_of(&current)),
                    );
                    Err(Duplicate)
                }
            }
            _ => {
                st.owner.insert(uid, src.to_string());
                st.reported.remove(src);
                drop(st);
                self.store.set_topology(src, nodes, edges);
                Ok(())
            }
        }
    }

    fn remove_source(&self, src: &str) {
        {
            let mut st = self.state();
            st.owner.retain(|_, owner| owner != src);
            st.reported.remove(src);
        }
        self.store.remove_source(src);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/dedupe.rs"]
mod tests;
