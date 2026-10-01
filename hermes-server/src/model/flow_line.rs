use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One line of the "who talks to whom" list: the busiest connections of a source in the last interval, with what the hub could tell of each end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FlowLine {
    /// The id of the node that asked (a pod, a Service, a host), or its address when the cluster does not know it.
    pub src: String,
    /// The same for what it asked for.
    pub dst: String,
    /// The id of the pod that answered, when `dst` is a Service; empty otherwise.
    pub via: String,
    pub port: u32,
    /// Mbit/s, both directions added.
    pub mbps: f64,
    /// `src` is an address the cluster does not know: someone from outside.
    pub external: bool,
}
