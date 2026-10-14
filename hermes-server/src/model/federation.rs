use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MEMBER_WAITING: &str = "waiting";
pub const MEMBER_PAIRED: &str = "paired";

/// What a hub that is managed by another one remembers. Only the hash of the pairing code and of the link token are kept: neither can be
/// read back, so a copy of the database does not let anyone take the place of the master.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MemberConfig {
    /// `waiting` (a code is shown, no master yet) or `paired`.
    pub state: String,
    pub code_hash: String,
    pub code_expires: i64,
    pub token_hash: String,
    /// Where the agents of this datacenter reach this hub: what the install manifests of its sources point at.
    pub agent_address: String,
    /// What the master hub calls itself.
    pub master: String,
    /// The name the master gave this datacenter.
    pub datacenter: String,
    pub paired_at: i64,
}

impl MemberConfig {
    pub fn is_paired(&self) -> bool {
        self.state == MEMBER_PAIRED
    }
}

/// A member hub the master controls. The token is encrypted at rest and never sent to the browser.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Datacenter {
    pub id: String,
    pub name: String,
    /// `https://hermes.dc2.example:8765` or `http://10.0.0.5:8765`.
    pub address: String,
    pub token: String,
}

/// The federation settings of one hub: the datacenters it controls, or the master that controls it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FederationConfig {
    /// What this hub is called next to the datacenters it controls.
    pub name: String,
    pub member: Option<MemberConfig>,
    pub datacenters: Vec<Datacenter>,
}

pub const DEFAULT_NAME: &str = "dc1";

impl FederationConfig {
    pub fn name(&self) -> &str {
        if self.name.trim().is_empty() {
            DEFAULT_NAME
        } else {
            &self.name
        }
    }
}

/// What a managed hub says about itself to its own browser: the pairing step, or that it is locked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemberView {
    /// `waiting` or `paired`.
    pub state: String,
    /// What the hub that manages this one calls itself, once paired.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub managed_by: Option<String>,
    /// The name that hub gave this one (this datacenter), once paired.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub datacenter: Option<String>,
    pub agent_address: String,
    /// When the pairing code stops working (ms since the epoch), while waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub code_expires: Option<i64>,
}

/// A datacenter as the master's browser sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DatacenterView {
    pub id: String,
    pub name: String,
    pub address: String,
    /// `connecting`, `connected` or `unreachable`.
    pub state: String,
    /// Why it is not connected, or what it is doing.
    pub info: String,
    /// Seconds since the member last said anything, while connected or after it was lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub seen_ago: Option<u64>,
    pub version: String,
    /// Where the agents of this datacenter reach it: the address an install manifest of a source there has to use.
    pub agent_address: String,
    pub sources: usize,
}

/// `GET /api/federation`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FederationView {
    /// What this hub is called next to the datacenters it controls.
    pub name: String,
    pub member: Option<MemberView>,
    pub datacenters: Vec<DatacenterView>,
}
