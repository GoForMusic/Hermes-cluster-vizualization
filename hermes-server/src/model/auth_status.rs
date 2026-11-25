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
    /// What the person may do (none when nobody is logged in; empty when they may do nothing): rules in the form of the policies, so the page can leave out what would be refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub grants: Option<Vec<crate::model::Grant>>,
    /// The identity providers a person who is not logged in may sign in with.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sso: Option<Vec<crate::model::SsoProviderView>>,
    /// This hub is managed by another one (or is about to be): it has no login of its own, and the page says so.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub member: Option<MemberView>,
}
