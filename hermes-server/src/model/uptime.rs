use serde::Serialize;
use ts_rs::TS;

/// The Kuma-style history of one node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Uptime {
    pub bars: Vec<String>, // per bucket: up | warn | down | nodata
    pub pct: f64,          // share of observed buckets without downtime
    pub current: String,
    #[ts(type = "number")]
    pub first: i64, // when the hub first saw this node (ms): the age of its history
}
