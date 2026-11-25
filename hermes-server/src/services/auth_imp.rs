//! The admin login: first-run setup, sessions and password changes. `IAuth` is the interface `controller` depends on;
//! `AuthImp` is its implementation.
//!
//! Passwords are argon2id in the PHC string format (`$argon2id$v=19$m=…,t=…,p=…$salt$hash`), the same the Go hub wrote, so an existing
//! account keeps working. The parameters travel inside the string, so they can be raised later without invalidating old passwords.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::database::{IGroupDAO, ISessionDAO, ISettingsDAO, IUserDAO, NewUser, now_ms};
use crate::model::{SessionPolicy, User};

pub(crate) const MIN_PASSWORD: usize = 10;
const PUBLIC_VIEW_KEY: &str = "auth.public_view";
const SESSION_POLICY_KEY: &str = "auth.session_policy";
/// A session is marked as used at most this often: a page that asks every second does not write to the database every second.
const TOUCH_EVERY_MS: i64 = 30_000;

// argon2id, the OWASP-recommended password hash
const MEMORY_KIB: u32 = 64 * 1024;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 2;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;

#[derive(Debug)]
pub enum AuthError {
    SetupDone,
    BadCredentials,
    RateLimited,
    BadUsername,
    WeakPassword,
    BadSessionPolicy,
    Internal(anyhow::Error),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SetupDone => "an admin already exists",
            Self::BadCredentials => "wrong username or password",
            Self::RateLimited => "too many attempts, try again in a few minutes",
            Self::BadUsername => {
                "username must be 3-32 characters: letters, digits, dot, dash or underscore"
            }
            Self::WeakPassword => "password must be at least 10 characters",
            Self::BadSessionPolicy => "a login lasts 1 hour to 30 days, and the inactivity limit is off or 5 minutes to a day",
            Self::Internal(_) => "internal error",
        })
    }
}

impl From<anyhow::Error> for AuthError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

pub fn hash_password(password: &str) -> String {
    let salt: [u8; SALT_LEN] = rand::random();
    let key = derive(
        password.as_bytes(),
        &salt,
        MEMORY_KIB,
        ITERATIONS,
        PARALLELISM,
        KEY_LEN,
    )
    .expect("the built-in argon2 parameters are valid");
    format!(
        "$argon2id$v=19$m={MEMORY_KIB},t={ITERATIONS},p={PARALLELISM}${}${}",
        STANDARD_NO_PAD.encode(salt),
        STANDARD_NO_PAD.encode(key)
    )
}

fn derive(password: &[u8], salt: &[u8], m: u32, t: u32, p: u32, len: usize) -> Option<Vec<u8>> {
    let params = Params::new(m, t, p, Some(len)).ok()?;
    let mut out = vec![0u8; len];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password, salt, &mut out)
        .ok()?;
    Some(out)
}

/// Compares in constant time. A malformed hash never verifies.
pub fn verify_password(password: &str, encoded: &str) -> bool {
    let parts: Vec<&str> = encoded.split('$').collect();
    let [_, "argon2id", "v=19", params, salt, want] = parts[..] else {
        return false;
    };
    let mut nums = params
        .split(',')
        .map(|kv| kv.split_once('=').and_then(|(_, v)| v.parse::<u32>().ok()));
    let (Some(Some(m)), Some(Some(t)), Some(Some(p)), None) =
        (nums.next(), nums.next(), nums.next(), nums.next())
    else {
        return false;
    };
    let (Ok(salt), Ok(want)) = (STANDARD_NO_PAD.decode(salt), STANDARD_NO_PAD.decode(want)) else {
        return false;
    };
    if want.is_empty() || m > 1 << 20 {
        return false; // refuse absurd memory settings from a corrupted hash
    }
    derive(password.as_bytes(), &salt, m, t, p, want.len())
        .is_some_and(|got| bool::from(got.ct_eq(&want)))
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn valid_username(name: &str) -> bool {
    (3..=32).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

pub(crate) fn normalize(username: &str) -> Result<String, AuthError> {
    let name = username.trim().to_lowercase();
    if valid_username(&name) {
        Ok(name)
    } else {
        Err(AuthError::BadUsername)
    }
}

const MAX_FAILS: usize = 5;
const WINDOW_MS: i64 = 10 * 60 * 1000;

/// Blocks a key after too many failures inside a window. In memory: a restart clears it, which is fine for slowing down guessing.
#[derive(Default)]
struct Limiter {
    fails: HashMap<String, Vec<i64>>,
}

impl Limiter {
    fn recent(&mut self, key: &str, now: i64) -> usize {
        let Some(list) = self.fails.get_mut(key) else {
            return 0;
        };
        list.retain(|t| now - t < WINDOW_MS);
        let n = list.len();
        if n == 0 {
            self.fails.remove(key);
        }
        n
    }
}

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// What `controller` depends on: nothing here mentions SQLite, hashing algorithms or rate-limit bookkeeping.
pub trait IAuth: Send + Sync {
    fn needs_setup(&self) -> Result<bool, AuthError>;
    /// Creates the first admin and logs them in. It only works while there is no user at all.
    fn setup(&self, username: &str, password: &str, public_view: bool)
    -> Result<String, AuthError>;
    /// `ip` and the username together are what the rate limit counts.
    fn login(&self, username: &str, password: &str, ip: &str) -> Result<String, AuthError>;
    fn logout(&self, token: &str);
    /// Resolves a cookie token to its user, or `None` (unknown, expired, or idle for too long). A call that finds the session alive marks it used.
    fn authenticate(&self, token: &str) -> Option<User>;
    /// Whether the session is still good, without marking it used: what a stream that the page keeps open asks.
    fn is_alive(&self, token: &str) -> bool;
    /// How long a login lasts.
    fn session_policy(&self) -> SessionPolicy;
    fn set_session_policy(&self, policy: SessionPolicy) -> Result<(), AuthError>;
    /// Checks the current password, sets the new one, ends every session of that user and returns a fresh session token for the caller.
    fn change_password(&self, user_id: i64, current: &str, next: &str)
    -> Result<String, AuthError>;
    /// May the read-only wallboard be seen without logging in?
    fn public_view(&self) -> bool;
    fn set_public_view(&self, on: bool) -> Result<(), AuthError>;
    /// Deletes sessions that ran out.
    fn purge_expired(&self);
}

pub struct AuthImp {
    users: Arc<dyn IUserDAO>,
    groups: Arc<dyn IGroupDAO>,
    sessions: Arc<dyn ISessionDAO>,
    settings: Arc<dyn ISettingsDAO>,
    now: Clock,
    limiter: Mutex<Limiter>,
    /// One first-run setup at a time.
    setup: Mutex<()>,
    /// Verified against when the user does not exist, so that timing does not reveal usernames.
    dummy: String,
}

impl AuthImp {
    pub fn new(
        users: Arc<dyn IUserDAO>,
        groups: Arc<dyn IGroupDAO>,
        sessions: Arc<dyn ISessionDAO>,
        settings: Arc<dyn ISettingsDAO>,
    ) -> Self {
        Self::with_clock(users, groups, sessions, settings, Arc::new(now_ms))
    }

    pub fn with_clock(
        users: Arc<dyn IUserDAO>,
        groups: Arc<dyn IGroupDAO>,
        sessions: Arc<dyn ISessionDAO>,
        settings: Arc<dyn ISettingsDAO>,
        now: Clock,
    ) -> Self {
        Self {
            users,
            groups,
            sessions,
            settings,
            now,
            limiter: Mutex::default(),
            setup: Mutex::default(),
            dummy: hash_password("not-a-real-password"),
        }
    }

    fn limiter(&self) -> std::sync::MutexGuard<'_, Limiter> {
        self.limiter.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn blocked(&self, key: &str) -> bool {
        self.limiter().recent(key, (self.now)()) >= MAX_FAILS
    }

    fn fail(&self, key: &str) {
        let now = (self.now)();
        let mut l = self.limiter();
        l.recent(key, now);
        l.fails.entry(key.to_string()).or_default().push(now);
        // A key that fails once and is never retried would otherwise sit here forever: nothing else ever looks it up
        // again to trigger `recent`'s own cleanup. Piggyback a sweep of the whole map on every failure instead of a
        // background task — this stays a login attempt, not a hot path, so the O(keys) pass costs nothing that matters.
        l.fails.retain(|_, list| {
            list.retain(|t| now - t < WINDOW_MS);
            !list.is_empty()
        });
    }

    fn reset(&self, key: &str) {
        self.limiter().fails.remove(key);
    }

    fn new_session(&self, user_id: i64) -> Result<String, AuthError> {
        let raw: [u8; 32] = rand::random();
        let token = URL_SAFE_NO_PAD.encode(raw);
        let now = (self.now)();
        self.sessions.create_session(
            &hash_token(&token),
            user_id,
            now,
            now + self.session_policy().max_ms(),
        )?;
        Ok(token)
    }
}

impl AuthImp {
    /// The session behind a token, if it is still good under the policy of now: not past its end, not older than the longest login, not idle
    /// for longer than the limit. One that is not is deleted. `touch`: it was used just now.
    fn check(&self, token: &str, touch: bool) -> Option<User> {
        if token.is_empty() {
            return None;
        }
        let hash = hash_token(token);
        let row = self.sessions.session(&hash).ok().flatten()?;
        let now = (self.now)();
        let policy = self.session_policy();
        // a session from before `created` was kept has only its end, which was set from the 14 days of the time
        let too_old = row.created > 0 && now - row.created > policy.max_ms();
        let idle = policy
            .idle_ms()
            .is_some_and(|limit| row.last_seen > 0 && now - row.last_seen > limit);
        if row.expires < now || too_old || idle {
            let _ = self.sessions.delete_session(&hash);
            return None;
        }
        let user = self
            .users
            .user_by_id(row.user_id)
            .ok()
            .flatten()
            .filter(|u| !u.disabled)?;
        if touch && now - row.last_seen >= TOUCH_EVERY_MS {
            let _ = self.sessions.touch_session(&hash, now);
        }
        Some(user)
    }
}

impl IAuth for AuthImp {
    fn needs_setup(&self) -> Result<bool, AuthError> {
        Ok(self.users.count_users()? == 0)
    }

    fn setup(
        &self,
        username: &str,
        password: &str,
        public_view: bool,
    ) -> Result<String, AuthError> {
        let _one_at_a_time = self.setup.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.needs_setup()? {
            return Err(AuthError::SetupDone);
        }
        let name = normalize(username)?;
        if password.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        // the first account is the break-glass one: it keeps its password whatever identity providers are added later
        let user = self.users.create_user(&NewUser {
            username: &name,
            password_hash: &hash_password(password),
            role: "admin",
            email: "",
            break_glass: true,
            created: (self.now)(),
        })?;
        // and in `owners`, the group of the `admin` policy: what the first account was able to do before groups existed
        if let Some(owners) = self.groups.group_by_name("owners")? {
            let mut members: Vec<i64> = self
                .groups
                .memberships()?
                .into_iter()
                .filter(|(g, _)| *g == owners.id)
                .map(|(_, u)| u)
                .collect();
            members.push(user.id);
            self.groups.set_members(owners.id, &members)?;
        }
        self.set_public_view(public_view)?;
        self.new_session(user.id)
    }

    fn login(&self, username: &str, password: &str, ip: &str) -> Result<String, AuthError> {
        let key = format!("{ip}|{}", username.trim().to_lowercase());
        if self.blocked(&key) {
            return Err(AuthError::RateLimited);
        }
        let user = normalize(username)
            .ok()
            .and_then(|name| self.users.user_by_name(&name).ok().flatten());
        let ok = verify_password(
            password,
            user.as_ref().map_or(&self.dummy, |u| &u.password_hash),
        );
        // a disabled account is told the same as a wrong password: nothing here says that the name exists
        let Some(user) = user.filter(|u| ok && !u.disabled) else {
            self.fail(&key);
            return Err(AuthError::BadCredentials);
        };
        self.reset(&key);
        self.new_session(user.id)
    }

    fn logout(&self, token: &str) {
        let _ = self.sessions.delete_session(&hash_token(token));
    }

    fn authenticate(&self, token: &str) -> Option<User> {
        self.check(token, true)
    }

    fn is_alive(&self, token: &str) -> bool {
        self.check(token, false).is_some()
    }

    fn session_policy(&self) -> SessionPolicy {
        self.settings
            .get_setting(SESSION_POLICY_KEY)
            .and_then(|raw| serde_json::from_str::<SessionPolicy>(&raw).ok())
            .filter(SessionPolicy::valid)
            .unwrap_or(SessionPolicy::DEFAULT)
    }

    fn set_session_policy(&self, policy: SessionPolicy) -> Result<(), AuthError> {
        if !policy.valid() {
            return Err(AuthError::BadSessionPolicy);
        }
        let raw = serde_json::to_string(&policy).map_err(|e| AuthError::Internal(e.into()))?;
        Ok(self.settings.set_setting(SESSION_POLICY_KEY, &raw)?)
    }

    fn change_password(
        &self,
        user_id: i64,
        current: &str,
        next: &str,
    ) -> Result<String, AuthError> {
        let key = format!("pw|{user_id}");
        if self.blocked(&key) {
            return Err(AuthError::RateLimited);
        }
        let user = self
            .users
            .user_by_id(user_id)?
            .ok_or(AuthError::BadCredentials)?;
        if !verify_password(current, &user.password_hash) {
            self.fail(&key);
            return Err(AuthError::BadCredentials);
        }
        if next.chars().count() < MIN_PASSWORD {
            return Err(AuthError::WeakPassword);
        }
        self.users.set_password(user_id, &hash_password(next))?;
        self.reset(&key);
        self.sessions.delete_user_sessions(user_id)?;
        self.new_session(user_id)
    }

    fn public_view(&self) -> bool {
        self.settings.get_setting(PUBLIC_VIEW_KEY).as_deref() == Some("1")
    }

    fn set_public_view(&self, on: bool) -> Result<(), AuthError> {
        Ok(self
            .settings
            .set_setting(PUBLIC_VIEW_KEY, if on { "1" } else { "0" })?)
    }

    fn purge_expired(&self) {
        let _ = self.sessions.delete_expired_sessions((self.now)());
    }
}

#[cfg(test)]
#[path = "../../tests/unit/auth.rs"]
mod tests;
