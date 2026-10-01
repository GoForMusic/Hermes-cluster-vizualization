use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One line of the agent ↔ hub communication log, without its content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommLogRow {
    #[ts(type = "number")]
    pub id: u64,
    #[ts(type = "number")]
    pub ts: i64,
    pub source: String,
    pub source_name: String,
    pub agent: String,
    /// `in` (agent to hub), `out` (hub to agent) or `link` (a connection opened or closed).
    pub dir: String,
    /// `batch`, `upgrade_status`, `resync`, `upgrade`, `connected`, `disconnected`.
    pub kind: String,
    /// What is in it, in a few words: `snapshot ×1 · metrics ×1`.
    pub summary: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// The log as the admin page reads it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommLogView {
    pub enabled: bool,
    /// How many entries, and how many minutes back, the hub keeps at most.
    pub capacity: u32,
    pub minutes: u32,
    /// Empty batches (the once-a-second heartbeats) since the log was turned on: counted, not listed.
    #[ts(type = "number")]
    pub heartbeats: u64,
    /// Newest first.
    pub entries: Vec<CommLogRow>,
    /// The newest id ever recorded: ask for `since` this to get only what is newer.
    #[ts(type = "number")]
    pub newest: u64,
}

/// One entry with its content.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommLogEntry {
    #[serde(flatten)]
    pub row: CommLogRow,
    #[ts(type = "unknown")]
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct CommLogSwitch {
    pub enabled: bool,
}
