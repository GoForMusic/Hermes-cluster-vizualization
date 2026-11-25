//! Single sign-on: the identity providers an admin sets up (OpenID Connect: Microsoft Entra ID, Google, Keycloak, Okta, ...).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A provider as the hub holds it (the secret in the clear: it is encrypted only in the database).
#[derive(Debug, Clone, Default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub scopes: String,
    pub groups_claim: String,
    /// Comma-separated; empty is any.
    pub allowed_domains: String,
    pub enabled: bool,
    /// A person this provider vouches for who has no account gets one at their first login.
    pub auto_create: bool,
    /// A local account with the same email becomes theirs (it keeps its groups and what it acknowledged; the password goes).
    pub link_existing: bool,
    /// The provider's email claim is trusted even when it does not say it was verified (a company tenant: Microsoft Entra ID).
    pub trust_email: bool,
    /// While this provider is on, only break-glass accounts may log in with a password.
    pub enforce: bool,
    pub default_group: Option<i64>,
    pub created: i64,
}

/// What the login screen needs to offer a provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SsoProviderView {
    pub id: String,
    pub name: String,
}

/// A value of the provider's groups claim, and the group of this hub its holders are put in at every login.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupMapping {
    pub claim: String,
    #[ts(type = "number")]
    pub group: i64,
}

/// `GET /api/identity-providers`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    /// There is a secret saved (it is never sent back).
    pub has_secret: bool,
    pub scopes: String,
    pub groups_claim: String,
    pub allowed_domains: String,
    pub enabled: bool,
    pub auto_create: bool,
    pub link_existing: bool,
    pub trust_email: bool,
    pub enforce: bool,
    #[ts(type = "number | null")]
    pub default_group: Option<i64>,
    pub group_map: Vec<GroupMapping>,
    /// What to register at the provider as the redirect (callback) address of this hub.
    pub redirect_uri: String,
}

/// What the admin sends to add a provider or change one.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderInput {
    pub name: String,
    pub issuer: String,
    pub client_id: String,
    /// Empty on a change keeps the saved one.
    pub secret: String,
    pub scopes: String,
    pub groups_claim: String,
    pub allowed_domains: String,
    pub enabled: bool,
    pub auto_create: bool,
    pub link_existing: bool,
    pub trust_email: bool,
    pub enforce: bool,
    #[ts(type = "number | null")]
    pub default_group: Option<i64>,
    pub group_map: Vec<GroupMapping>,
}

/// `POST /api/identity-providers/test`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderTest {
    pub ok: bool,
    pub message: String,
}
