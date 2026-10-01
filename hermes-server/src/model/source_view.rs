use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::AgentView;
use crate::model::{Source, UpgradeView};

/// A source plus the agents that report for it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceView {
    #[serde(flatten)]
    pub source: Source,
    pub agents: Vec<AgentView>,
    /// The agent version the install manifests ask for; an agent below it is `outdated`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_agent: Option<String>,
    /// Some connected agent of this source was installed to change its own image: "Change agent version" works from the dashboard.
    pub can_upgrade: bool,
    /// The last version change that was asked for, and how it is going.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub upgrade: Option<UpgradeView>,
}
