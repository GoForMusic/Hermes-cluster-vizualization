use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::Source;

/// The answer to adding a source: the source and the manifest to install (shown once: it holds the token).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AddSourceResponse {
    #[serde(flatten)]
    pub source: Source,
    pub install: String,
    pub hint: String,
}
