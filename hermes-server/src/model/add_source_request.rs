use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What the browser sends to add a source.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct AddSourceRequest {
    pub name: String,
    #[serde(rename = "type")]
    #[ts(rename = "type")]
    pub kind: String,
    pub endpoint: String,
    /// The address of the hub as seen from the agent.
    pub hub_url: String,
    /// Kubernetes only: the node agents also report who talks to whom (they run in the node's network namespace).
    #[ts(as = "Option<bool>", optional)]
    pub flows: bool,
    /// The agent version to install, from the registry's list. Required once a registry is set up.
    #[ts(as = "Option<String>", optional)]
    pub version: String,
    /// The agents may change their own image when the dashboard asks (Change agent version). Off, the agent stays strictly read-only.
    #[ts(as = "Option<bool>", optional)]
    pub upgrades: bool,
    /// Swarm and Docker: some machines are Windows, so the stack (or compose file) also gets the Windows agent.
    #[ts(as = "Option<bool>", optional)]
    pub windows: bool,
}
