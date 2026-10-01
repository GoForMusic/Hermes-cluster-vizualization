use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/info`
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HubInfo {
    pub demo: bool,
    pub source_types: Vec<String>,
    pub version: String,
}
