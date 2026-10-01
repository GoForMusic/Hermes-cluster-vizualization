use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One agent of a source, as the admin sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentView {
    pub id: String,
    pub version: String,
    pub collector: String,
    pub host: String,
    #[ts(type = "number")]
    pub seen_ago: u64,
    pub outdated: bool,
    /// Speaks an older wire protocol than this hub (`Hello.protocol` below `hermes_proto::PROTOCOL`). Still accepted — the hub stays
    /// backward compatible — but the admin should know an upgrade is due.
    pub protocol_outdated: bool,
}
