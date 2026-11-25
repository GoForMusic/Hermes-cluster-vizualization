//! Someone who can log in to the admin panel, and the groups they are put in.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The hash never leaves the server.
#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub created: i64,
    pub email: String,
    /// Cannot log in and holds no session; the account and its groups stay.
    pub disabled: bool,
    /// A local account that always keeps its password and its access, whatever else is configured (the way in when the identity provider
    /// is down or set up wrong). Never disabled or deleted, and there is always at least one.
    pub break_glass: bool,
}

/// A set of users. What a group may do comes with the policies that are attached to it.
#[derive(Debug, Clone)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub created: i64,
}

/// `GET /api/users`: one account, without anything secret.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserView {
    #[ts(type = "number")]
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub disabled: bool,
    pub break_glass: bool,
    #[ts(type = "number")]
    pub created: i64,
    /// The ids of the groups this user is in.
    #[ts(type = "Array<number>")]
    pub groups: Vec<i64>,
}

/// `GET /api/groups`: a group and who is in it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupView {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    pub description: String,
    #[ts(type = "number")]
    pub created: i64,
    /// The ids of the users in this group.
    #[ts(type = "Array<number>")]
    pub members: Vec<i64>,
}
