use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where an agent upgrade a source was asked for stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpgradeView {
    /// The version that was asked for.
    pub version: String,
    /// `pending` (the cluster is replacing the agents), `done` (every agent runs it) or `failed`.
    pub state: String,
    /// Why it failed; empty otherwise.
    pub message: String,
}

/// What the browser sends to change a source's agent version.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct UpgradeRequest {
    pub version: String,
}
