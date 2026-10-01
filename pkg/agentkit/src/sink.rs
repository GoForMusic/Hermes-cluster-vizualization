use std::collections::HashMap;
use std::sync::Arc;

use hermes_proto::v1::{CollectorState, Edge, Flow, Node, Own};
use prost_types::Struct;

/// Where a collector describes the world it sees. In an agent it queues events for the uplink; collectors cannot tell.
pub trait ISink: Send + Sync {
    /// The whole topology. It replaces the previous one.
    fn set_topology(&self, nodes: Vec<Node>, edges: Vec<Edge>);
    fn status(&self, id: &str, own: Own, reason: &str);
    /// Keys in `patch` overwrite the node's old meta.
    fn meta(&self, id: &str, patch: Struct);
    fn metrics(&self, nodes: HashMap<String, HashMap<String, f64>>, edges: HashMap<String, f64>);
    /// The collector's own state. An agent that is alive but cannot reach its cluster must not look healthy.
    fn report(&self, state: CollectorState, info: &str);
    /// The workload nodes running on the host this collector runs on, as that host itself sees them. It keeps them shown as running
    /// when the control plane, the only other source of their state, cannot be reached.
    fn alive(&self, ids: Vec<String>);
    /// Nodes only this collector can see (the volumes of its own machine). Each call replaces the previous one; the hub adds them to the topology.
    fn contribute(&self, nodes: Vec<Node>);
    /// Who talked to whom on this host over the last interval. Only the newest matters. A collector that does not measure it never calls this.
    fn flows(&self, _flows: Vec<Flow>) {}
}

pub type SharedSink = Arc<dyn ISink>;
