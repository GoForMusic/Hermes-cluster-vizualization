//! An event of the live stream.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::model::Alert;
use crate::model::Edge;
use crate::model::FlowLine;
use crate::model::{Meta, Node};

/// An event of the live stream (`GET /api/stream`). The hub builds them with `json!`; a test checks that every one of them parses as this.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "lowercase")]
#[ts(export)]
pub enum HubEvent {
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
        #[ts(type = "Record<string, unknown>")]
        meta: Meta,
    },
    Metrics {
        nodes: HashMap<String, HashMap<String, f64>>,
        edges: HashMap<String, f64>,
    },
    Alert {
        alert: Alert,
        #[serde(rename = "isNew")]
        #[ts(rename = "isNew")]
        is_new: bool,
    },
    Sources,
    /// The busiest connections of one source (from the flows agents), newest first sample replaces the last.
    Flows {
        source: String,
        flows: Vec<FlowLine>,
    },
    Settings {
        #[ts(type = "Record<string, unknown>")]
        settings: Value,
    },
}
