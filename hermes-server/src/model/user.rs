//! Someone who can log in to the admin panel.

use serde::Serialize;

/// The hash never leaves the server.
#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub created: i64,
}
