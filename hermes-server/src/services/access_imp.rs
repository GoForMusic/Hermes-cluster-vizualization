//! Who can log in, and the groups they are in: the accounts the admin page manages. `IAccess` is the interface `controller` depends on;
//! `AccessImp` is its implementation. What an account may *do* is decided elsewhere; this is only who exists.
//!
//! Two rules keep the hub from locking everybody out: a break-glass account (a local account with a password, the way in when the identity
//! provider is down or misconfigured) is never disabled or deleted, and there is always at least one of them.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::database::{IGroupDAO, ISessionDAO, IUserDAO, NewUser, UserPatch, now_ms};
use crate::model::{Group, GroupView, User, UserView};
use crate::services::auth_imp::{MIN_PASSWORD, hash_password, normalize};

#[derive(Debug)]
pub enum AccessError {
    BadName,
    WeakPassword,
    BadEmail,
    BadGroupName,
    /// The username or the group name is in use.
    Taken,
    NotFound,
    /// The rule that stops it, as it is shown to the admin.
    Protected(&'static str),
    Internal(anyhow::Error),
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadName => {
                "username must be 3-32 characters: letters, digits, dot, dash or underscore"
            }
            Self::WeakPassword => "password must be at least 10 characters",
            Self::BadEmail => "that is not an email address",
            Self::BadGroupName => "a group name is 1-64 characters with no line breaks",
            Self::Taken => "that name is already in use",
            Self::NotFound => "not found",
            Self::Protected(why) => why,
            Self::Internal(_) => "internal error",
        })
    }
}

impl From<anyhow::Error> for AccessError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

/// What the admin changes about an account; `None` leaves it as it is.
#[derive(Default)]
pub struct UserChanges {
    pub email: Option<String>,
    pub disabled: Option<bool>,
    pub break_glass: Option<bool>,
    /// A new password, set by the admin; the account's sessions end.
    pub password: Option<String>,
}

pub trait IAccess: Send + Sync {
    fn users(&self) -> Result<Vec<UserView>, AccessError>;
    fn create_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
        break_glass: bool,
    ) -> Result<UserView, AccessError>;
    /// `acting` is the admin making the change (0 when it is a master hub): nobody disables, deletes or resets themselves from here.
    fn update_user(
        &self,
        acting: i64,
        id: i64,
        changes: UserChanges,
    ) -> Result<UserView, AccessError>;
    fn delete_user(&self, acting: i64, id: i64) -> Result<(), AccessError>;

    fn groups(&self) -> Result<Vec<GroupView>, AccessError>;
    fn create_group(&self, name: &str, description: &str) -> Result<GroupView, AccessError>;
    fn update_group(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<GroupView, AccessError>;
    fn delete_group(&self, id: i64) -> Result<(), AccessError>;
    /// Makes exactly these users the members of the group.
    fn set_members(&self, id: i64, users: &[i64]) -> Result<GroupView, AccessError>;
}

pub struct AccessImp {
    users: Arc<dyn IUserDAO>,
    groups: Arc<dyn IGroupDAO>,
    sessions: Arc<dyn ISessionDAO>,
    now: Arc<dyn Fn() -> i64 + Send + Sync>,
    /// The rules look at several accounts and then change one: one change at a time.
    write: Mutex<()>,
}

impl AccessImp {
    pub fn new(
        users: Arc<dyn IUserDAO>,
        groups: Arc<dyn IGroupDAO>,
        sessions: Arc<dyn ISessionDAO>,
    ) -> Self {
        Self {
            users,
            groups,
            sessions,
            now: Arc::new(now_ms),
            write: Mutex::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn user(&self, id: i64) -> Result<User, AccessError> {
        self.users.user_by_id(id)?.ok_or(AccessError::NotFound)
    }

    fn user_view(&self, user: &User) -> Result<UserView, AccessError> {
        let groups = self
            .groups
            .memberships()?
            .into_iter()
            .filter(|(_, u)| *u == user.id)
            .map(|(g, _)| g)
            .collect();
        Ok(UserView {
            id: user.id,
            username: user.username.clone(),
            email: user.email.clone(),
            role: user.role.clone(),
            disabled: user.disabled,
            break_glass: user.break_glass,
            created: user.created,
            groups,
        })
    }

    fn group_view(&self, group: &Group) -> Result<GroupView, AccessError> {
        let members = self
            .groups
            .memberships()?
            .into_iter()
            .filter(|(g, _)| *g == group.id)
            .map(|(_, u)| u)
            .collect();
        Ok(view_of(group, members))
    }

    /// Break-glass accounts that can still log in, other than `except`.
    fn other_break_glass(&self, except: i64) -> Result<usize, AccessError> {
        Ok(self
            .users
            .list_users()?
            .iter()
            .filter(|u| u.break_glass && !u.disabled && u.id != except)
            .count())
    }
}

fn view_of(group: &Group, members: Vec<i64>) -> GroupView {
    GroupView {
        id: group.id,
        name: group.name.clone(),
        description: group.description.clone(),
        created: group.created,
        members,
    }
}

fn clean_email(email: &str) -> Result<String, AccessError> {
    let email = email.trim();
    if email.is_empty() {
        return Ok(String::new());
    }
    let ok = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email
            .split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.ends_with('.'));
    if ok {
        Ok(email.to_string())
    } else {
        Err(AccessError::BadEmail)
    }
}

fn clean_group_name(name: &str) -> Result<String, AccessError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.contains(['\n', '\r']) {
        Err(AccessError::BadGroupName)
    } else {
        Ok(name.to_string())
    }
}

fn clean_description(text: &str) -> String {
    text.trim().chars().take(200).collect()
}

fn taken_or(e: anyhow::Error) -> AccessError {
    // the only way these inserts and renames fail on purpose is the UNIQUE constraint on the name
    match e.downcast_ref::<rusqlite::Error>() {
        Some(rusqlite::Error::SqliteFailure(f, _))
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AccessError::Taken
        }
        _ => AccessError::Internal(e),
    }
}

impl IAccess for AccessImp {
    fn users(&self) -> Result<Vec<UserView>, AccessError> {
        let members = self.groups.memberships()?;
        let mut by_user: HashMap<i64, Vec<i64>> = HashMap::new();
        for (g, u) in members {
            by_user.entry(u).or_default().push(g);
        }
        Ok(self
            .users
            .list_users()?
            .into_iter()
            .map(|u| UserView {
                groups: by_user.remove(&u.id).unwrap_or_default(),
                id: u.id,
                username: u.username,
                email: u.email,
                role: u.role,
                disabled: u.disabled,
                break_glass: u.break_glass,
                created: u.created,
            })
            .collect())
    }

    fn create_user(
        &self,
        username: &str,
        password: &str,
        email: &str,
        break_glass: bool,
    ) -> Result<UserView, AccessError> {
        let name = normalize(username).map_err(|_| AccessError::BadName)?;
        if password.chars().count() < MIN_PASSWORD {
            return Err(AccessError::WeakPassword);
        }
        let email = clean_email(email)?;
        let _one = self.lock();
        if self.users.user_by_name(&name)?.is_some() {
            return Err(AccessError::Taken);
        }
        let user = self.users.create_user(&NewUser {
            username: &name,
            password_hash: &hash_password(password),
            role: "admin",
            email: &email,
            break_glass,
            created: (self.now)(),
        })?;
        self.user_view(&user)
    }

    fn update_user(
        &self,
        acting: i64,
        id: i64,
        changes: UserChanges,
    ) -> Result<UserView, AccessError> {
        let email = changes.email.as_deref().map(clean_email).transpose()?;
        if changes
            .password
            .as_ref()
            .is_some_and(|p| p.chars().count() < MIN_PASSWORD)
        {
            return Err(AccessError::WeakPassword);
        }
        let _one = self.lock();
        let user = self.user(id)?;
        if changes.disabled == Some(true) && !user.disabled {
            if id == acting {
                return Err(AccessError::Protected(
                    "you cannot disable your own account",
                ));
            }
            if user.break_glass {
                return Err(AccessError::Protected(
                    "a break-glass account cannot be disabled: take the flag off first",
                ));
            }
        }
        if changes.break_glass == Some(false)
            && user.break_glass
            && !user.disabled
            && self.other_break_glass(id)? == 0
        {
            return Err(AccessError::Protected(
                "there must be at least one break-glass account: make another one first",
            ));
        }
        if changes.break_glass == Some(true) && user.disabled && changes.disabled != Some(false) {
            return Err(AccessError::Protected(
                "a disabled account cannot be a break-glass account",
            ));
        }
        if changes.password.is_some() && id == acting {
            return Err(AccessError::Protected(
                "change your own password in Settings → Profile",
            ));
        }
        self.users.update_user(
            id,
            &UserPatch {
                email,
                disabled: changes.disabled,
                break_glass: changes.break_glass,
            },
        )?;
        if let Some(password) = &changes.password {
            self.users.set_password(id, &hash_password(password))?;
        }
        if changes.disabled == Some(true) || changes.password.is_some() {
            self.sessions.delete_user_sessions(id)?;
        }
        let user = self.user(id)?;
        self.user_view(&user)
    }

    fn delete_user(&self, acting: i64, id: i64) -> Result<(), AccessError> {
        let _one = self.lock();
        let user = self.user(id)?;
        if id == acting {
            return Err(AccessError::Protected("you cannot delete your own account"));
        }
        if user.break_glass {
            return Err(AccessError::Protected(
                "a break-glass account cannot be deleted: take the flag off first",
            ));
        }
        Ok(self.users.delete_user(id)?)
    }

    fn groups(&self) -> Result<Vec<GroupView>, AccessError> {
        let mut members: HashMap<i64, Vec<i64>> = HashMap::new();
        for (g, u) in self.groups.memberships()? {
            members.entry(g).or_default().push(u);
        }
        Ok(self
            .groups
            .list_groups()?
            .iter()
            .map(|g| view_of(g, members.remove(&g.id).unwrap_or_default()))
            .collect())
    }

    fn create_group(&self, name: &str, description: &str) -> Result<GroupView, AccessError> {
        let name = clean_group_name(name)?;
        let _one = self.lock();
        let group = self
            .groups
            .create_group(&name, &clean_description(description), (self.now)())
            .map_err(taken_or)?;
        Ok(view_of(&group, vec![]))
    }

    fn update_group(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<GroupView, AccessError> {
        let name = name.map(clean_group_name).transpose()?;
        let description = description.map(clean_description);
        let _one = self.lock();
        self.groups.group(id)?.ok_or(AccessError::NotFound)?;
        self.groups
            .update_group(id, name.as_deref(), description.as_deref())
            .map_err(taken_or)?;
        let group = self.groups.group(id)?.ok_or(AccessError::NotFound)?;
        self.group_view(&group)
    }

    fn delete_group(&self, id: i64) -> Result<(), AccessError> {
        let _one = self.lock();
        self.groups.group(id)?.ok_or(AccessError::NotFound)?;
        Ok(self.groups.delete_group(id)?)
    }

    fn set_members(&self, id: i64, users: &[i64]) -> Result<GroupView, AccessError> {
        let _one = self.lock();
        let group = self.groups.group(id)?.ok_or(AccessError::NotFound)?;
        let existing: Vec<i64> = self.users.list_users()?.iter().map(|u| u.id).collect();
        if users.iter().any(|u| !existing.contains(u)) {
            return Err(AccessError::NotFound);
        }
        self.groups.set_members(id, users)?;
        self.group_view(&group)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/access.rs"]
mod tests;
