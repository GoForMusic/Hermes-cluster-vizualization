//! A link between two nodes, and the protobuf conversion that builds one.

use hermes_proto::v1 as pb;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A traffic link (`type` "traffic"), a control-plane link ("control") or a route with no measured rate ("route").
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub base: f64,
    pub mbps: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

impl From<pb::Edge> for Edge {
    fn from(e: pb::Edge) -> Self {
        let kind = match e.r#type() {
            pb::EdgeType::Unspecified => "",
            pb::EdgeType::Traffic => "traffic",
            pb::EdgeType::Control => "control",
            pb::EdgeType::Route => "route",
        };
        Edge {
            kind: kind.to_string(),
            id: e.id,
            from: e.from,
            to: e.to,
            base: e.base,
            mbps: e.mbps,
        }
    }
}
