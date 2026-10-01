//! The hub's persistence: settings, sources, alert history, users, sessions and heartbeats, in one SQLite file today.
//! One interface per entity (`IAlertDAO`, `ISourceDAO`, ...), each with its own SQLite-backed implementation — every
//! other module depends on the interface it needs, never on SQLite directly, so the storage backend can change without
//! touching `services`, `controller` or anywhere else. `Repositories` is just the composition root that wires the seven
//! DAOs to one shared `IDbContext` at startup (see `main.rs`).

mod alert_dao_imp;
mod beat_dao_imp;
mod db_context_sqlite;
mod i_db_context;
mod registry_dao_imp;
mod session_dao_imp;
mod settings_dao_imp;
mod source_dao_imp;
mod user_dao_imp;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

pub use alert_dao_imp::{AlertDAOImp, IAlertDAO};
pub use beat_dao_imp::{BeatDAOImp, IBeatDAO};
pub use db_context_sqlite::DBContext_SQLite;
#[cfg(test)]
pub(crate) use db_context_sqlite::SCHEMA;
pub use i_db_context::IDbContext;
pub use registry_dao_imp::{IRegistryDAO, RegistryDAOImp};
pub use session_dao_imp::{ISessionDAO, SessionDAOImp};
pub use settings_dao_imp::{ISettingsDAO, SettingsDAOImp};
pub use source_dao_imp::{ISourceDAO, SourceDAOImp};
pub use user_dao_imp::{IUserDAO, UserDAOImp};

use crate::crypto::Key;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The seven DAOs the rest of the hub depends on, all sharing one `IDbContext`. Built once at startup (`sqlite`/
/// `sqlite_in_memory`); everywhere else takes the specific `Arc<dyn I*DAO>` it needs off this, never the whole thing.
pub struct Repositories {
    pub alerts: Arc<dyn IAlertDAO>,
    pub sources: Arc<dyn ISourceDAO>,
    pub beats: Arc<dyn IBeatDAO>,
    pub settings: Arc<dyn ISettingsDAO>,
    pub registry: Arc<dyn IRegistryDAO>,
    pub users: Arc<dyn IUserDAO>,
    pub sessions: Arc<dyn ISessionDAO>,
    /// The shared context every DAO above is built on. Only meant for tests that need to reach the raw connection
    /// (to set up a fixture SQLite would not let a DAO's own interface express) — everything else goes through a DAO.
    pub ctx: Arc<dyn IDbContext>,
}

impl Repositories {
    pub fn sqlite(path: &Path, key: Key) -> Result<Self> {
        Ok(Self::from_context(Arc::new(DBContext_SQLite::open(
            path, key,
        )?)))
    }

    /// A key that dies with the process: fine for tests, which never need to read what a previous run wrote.
    pub fn sqlite_in_memory() -> Result<Self> {
        Ok(Self::from_context(Arc::new(
            DBContext_SQLite::open_in_memory()?,
        )))
    }

    fn from_context(ctx: Arc<dyn IDbContext>) -> Self {
        Self {
            alerts: Arc::new(AlertDAOImp::new(ctx.clone())),
            sources: Arc::new(SourceDAOImp::new(ctx.clone())),
            beats: Arc::new(BeatDAOImp::new(ctx.clone())),
            settings: Arc::new(SettingsDAOImp::new(ctx.clone())),
            registry: Arc::new(RegistryDAOImp::new(ctx.clone())),
            users: Arc::new(UserDAOImp::new(ctx.clone())),
            sessions: Arc::new(SessionDAOImp::new(ctx.clone())),
            ctx,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/database.rs"]
mod tests;
