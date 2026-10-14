use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::MemberView;

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
    /// This hub is managed by another one (or is about to be): it has no login of its own, and the page says so.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub member: Option<MemberView>,
}
