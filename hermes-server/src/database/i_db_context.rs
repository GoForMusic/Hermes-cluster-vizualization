//! The one thing every DAO needs from whatever database backs it: a connection to work with and the key that encrypts
//! secrets at rest. A concrete context (`DBContext_SQLite` today; a `DBContext_MongoDB` or similar later) implements this;
//! every `*DAOImpl` is written against the interface, not the concrete context, so the storage backend can change under them.

use std::sync::MutexGuard;

use rusqlite::Connection;

use crate::crypto::Key;

pub trait IDbContext: Send + Sync {
    fn conn(&self) -> MutexGuard<'_, Connection>;
    fn key(&self) -> &Key;
}
