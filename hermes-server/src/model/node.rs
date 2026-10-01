//! A cluster, host, workload or volume, and the protobuf conversions that build one.

use std::collections::HashMap;

use hermes_proto::v1 as pb;
use hermes_proto::value::json_from_struct;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

pub type Meta = Map<String, Value>;

/// A cluster, host, workload or volume. `own` is the state its source reports; the browser derives the effective status from it
/// (everything on a crashed host becomes "unknown").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Node {
    pub id: String,
    pub kind: String, // cluster | host | workload | volume | network
    pub name: String,
    pub parent: Option<String>,
    pub provider: String, // kubernetes | swarm | nomad | storage
    pub own: String,      // ok | warn | crit
    pub status: String,
    pub reason: String,
    #[ts(type = "number")]
    pub since: i64,
    pub m: HashMap<String, f64>,
    #[ts(type = "Record<string, unknown>")]
    pub meta: Meta,
    /// The source that reports this node is unreachable, so `own` and `m` are the last known values, not current ones.
    #[serde(skip_serializing_if = "is_false")]
    #[ts(as = "Option<bool>", optional)]
    pub stale: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn kind_name(kind: pb::NodeKind) -> &'static str {
    match kind {
        pb::NodeKind::Unspecified => "",
        pb::NodeKind::Cluster => "cluster",
        pb::NodeKind::Host => "host",
        pb::NodeKind::Workload => "workload",
        pb::NodeKind::Volume => "volume",
        pb::NodeKind::Network => "network",
    }
}

fn provider_name(provider: pb::Provider) -> &'static str {
    match provider {
        pb::Provider::Unspecified => "",
        pb::Provider::Kubernetes => "kubernetes",
        pb::Provider::Swarm => "swarm",
        pb::Provider::Nomad => "nomad",
        pb::Provider::Storage => "storage",
    }
}

pub fn own_name(own: pb::Own) -> &'static str {
    match own {
        pb::Own::Unspecified => "",
        pb::Own::Ok => "ok",
        pb::Own::Warn => "warn",
        pb::Own::Crit => "crit",
    }
}

impl From<pb::Node> for Node {
    fn from(n: pb::Node) -> Self {
        let own = own_name(n.own()).to_string();
        Node {
            kind: kind_name(n.kind()).to_string(),
            provider: provider_name(n.provider()).to_string(),
            status: own.clone(),
            own,
            meta: match n.meta.as_ref().map(json_from_struct) {
                Some(Value::Object(map)) => map,
                _ => Meta::new(),
            },
            id: n.id,
            name: n.name,
            parent: n.parent,
            reason: n.reason,
            since: n.since,
            m: n.m,
            stale: false,
        }
    }
}
