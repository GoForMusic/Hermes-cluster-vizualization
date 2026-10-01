use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/auth/status`: who is looking.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuthStatus {
    pub setup_required: bool,
    pub authenticated: bool,
    pub public_view: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
}
