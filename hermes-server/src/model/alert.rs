use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How bad an alert is. Only two levels exist today — a rule either raises a warning or a critical incident.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Severity {
    #[default]
    // structurally required by Alert's #[serde(default)]; sev is always set explicitly, never meaningfully "defaulted"
    Warn,
    Crit,
}

/// A persisted incident; `resolved_ts` is empty while it is active.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Alert {
    #[ts(type = "number")]
    pub id: i64,
    pub key: String,
    pub sev: Severity,
    pub node_id: String,
    pub title: String,
    pub detail: String,
    #[ts(type = "number")]
    pub ts: i64,
    #[ts(type = "number | null")]
    pub resolved_ts: Option<i64>,
    pub ack: bool,
    /// Who acknowledged it and when; empty for one that was not, or was acknowledged before this was recorded.
    #[serde(skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub ack_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(type = "number", optional)]
    pub ack_ts: Option<i64>,
    /// What the node looked like when this alert opened (its own status, metrics and meta — image, restarts, containers — as JSON),
    /// captured once and never updated: the node itself may be gone by the time someone looks back at this. Empty for alerts that
    /// were never tied to one node's state (a whole source being unreachable) or opened before this existed.
    pub snapshot: String,
}
