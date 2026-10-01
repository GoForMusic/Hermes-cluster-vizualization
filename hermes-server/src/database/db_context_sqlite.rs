//! The SQLite `IDbContext`: opens the file (or an in-memory database for tests), runs the schema and one-time
//! migrations, and hands out the connection every DAO shares.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::crypto::{self, Key};
use crate::database::IDbContext;
use crate::database::now_ms;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sources (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, type TEXT NOT NULL, endpoint TEXT NOT NULL DEFAULT '',
  auth TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'pending', info TEXT NOT NULL DEFAULT '',
  secret TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS alerts (
  id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL, sev TEXT NOT NULL, node_id TEXT NOT NULL,
  title TEXT NOT NULL, detail TEXT NOT NULL, ts INTEGER NOT NULL, resolved_ts INTEGER, ack INTEGER NOT NULL DEFAULT 0,
  snapshot TEXT NOT NULL DEFAULT '', ack_by TEXT NOT NULL DEFAULT '', ack_ts INTEGER);
CREATE INDEX IF NOT EXISTS idx_alerts_ts ON alerts(ts DESC);
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT, username TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'admin', created INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (token_hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL, expires INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS beats (node_id TEXT NOT NULL, ts INTEGER NOT NULL, status TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_beats ON beats(node_id, ts);
";

/// Heartbeats are kept this long.
const BEATS_KEPT: Duration = Duration::from_secs(90 * 24 * 3600);

// The user's own naming convention for a swappable storage backend (`DBContext_<technology>`; a `DBContext_MongoDB`
// or similar would sit next to this one, also implementing `IDbContext`) — not idiomatic Rust casing, kept anyway.
#[allow(non_camel_case_types)]
pub struct DBContext_SQLite {
    conn: Mutex<Connection>,
    key: Key,
}

impl DBContext_SQLite {
    pub fn open(path: &Path, key: Key) -> Result<Self> {
        let conn =
            Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn, key)
    }

    /// A key that dies with the process: fine for tests, which never need to read what a previous run wrote.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, Key::random())
    }

    fn init(conn: Connection, key: Key) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA).context("migrate")?;
        // `alerts.snapshot` is in SCHEMA for a brand new database; an older one already has the table without it, so `CREATE TABLE IF
        // NOT EXISTS` above left it as it was. This adds the column there; on a fresh database it already exists, so the error (there
        // is no "ADD COLUMN IF NOT EXISTS") is expected and discarded.
        let _ = conn.execute(
            "ALTER TABLE alerts ADD COLUMN snapshot TEXT NOT NULL DEFAULT ''",
            [],
        );
        // the same for who acknowledged an alert, and when
        for column in [
            "ALTER TABLE alerts ADD COLUMN ack_by TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE alerts ADD COLUMN ack_ts INTEGER",
        ] {
            let _ = conn.execute(column, []);
        }
        let old = now_ms() - i64::try_from(BEATS_KEPT.as_millis()).unwrap_or(i64::MAX);
        conn.execute("DELETE FROM beats WHERE ts < ?1", [old])?;
        let ctx = Self {
            conn: Mutex::new(conn),
            key,
        };
        ctx.migrate_secrets()
            .context("encrypt secrets left in clear by an older hub")?;
        Ok(ctx)
    }

    /// One-time: encrypts any `sources.secret` still in clear (a database from before this existed, or from the Go
    /// hub). Safe to run on every startup: rows already encrypted are left untouched.
    fn migrate_secrets(&self) -> Result<()> {
        // One connection, held for the whole pass: nothing else can be using the database yet (this runs inside
        // `init`, before the context is shared), so there is no point releasing the lock between rows.
        let conn = self.conn();
        let plain: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT id, secret FROM sources")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<(String, String)>>>()?
        }
        .into_iter()
        .filter(|(_, secret)| crypto::is_plaintext(secret))
        .collect();
        for (id, secret) in plain {
            conn.execute(
                "UPDATE sources SET secret=?1 WHERE id=?2",
                params![crypto::encrypt(&self.key, &secret), id],
            )?;
        }
        Ok(())
    }
}

impl IDbContext for DBContext_SQLite {
    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn key(&self) -> &Key {
        &self.key
    }
}
