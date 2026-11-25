//! Login sessions: a separate table and lifecycle (expiry sweep, bulk delete by user) from `IUserDAO`'s `users` table.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::database::IDbContext;

pub trait ISessionDAO: Send + Sync {
    fn create_session(
        &self,
        token_hash: &str,
        user_id: i64,
        created_ms: i64,
        expires_ms: i64,
    ) -> Result<()>;
    fn session(&self, token_hash: &str) -> Result<Option<SessionRow>>;
    /// The session was used at this time (what the inactivity limit counts from).
    fn touch_session(&self, token_hash: &str, now_ms: i64) -> Result<()>;
    fn delete_session(&self, token_hash: &str) -> Result<()>;
    fn delete_user_sessions(&self, user_id: i64) -> Result<()>;
    fn delete_expired_sessions(&self, now_ms: i64) -> Result<()>;
}

/// A session as stored. `created` is 0 for one made before it was kept (it then only has its `expires`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRow {
    pub user_id: i64,
    pub created: i64,
    pub expires: i64,
    pub last_seen: i64,
}

pub struct SessionDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl SessionDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

impl ISessionDAO for SessionDAOImp {
    fn create_session(
        &self,
        token_hash: &str,
        user_id: i64,
        created_ms: i64,
        expires_ms: i64,
    ) -> Result<()> {
        self.ctx.conn().execute(
            "INSERT INTO sessions(token_hash,user_id,expires,created,last_seen) VALUES(?1,?2,?3,?4,?4)",
            params![token_hash, user_id, expires_ms, created_ms],
        )?;
        Ok(())
    }

    fn session(&self, token_hash: &str) -> Result<Option<SessionRow>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                "SELECT user_id,created,expires,MAX(last_seen,created) FROM sessions WHERE token_hash=?1",
                [token_hash],
                |r| {
                    Ok(SessionRow {
                        user_id: r.get(0)?,
                        created: r.get(1)?,
                        expires: r.get(2)?,
                        last_seen: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }

    fn touch_session(&self, token_hash: &str, now_ms: i64) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE sessions SET last_seen=?1 WHERE token_hash=?2",
            params![now_ms, token_hash],
        )?;
        Ok(())
    }

    fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.ctx
            .conn()
            .execute("DELETE FROM sessions WHERE token_hash=?1", [token_hash])?;
        Ok(())
    }

    fn delete_user_sessions(&self, user_id: i64) -> Result<()> {
        self.ctx
            .conn()
            .execute("DELETE FROM sessions WHERE user_id=?1", [user_id])?;
        Ok(())
    }

    fn delete_expired_sessions(&self, now_ms: i64) -> Result<()> {
        self.ctx
            .conn()
            .execute("DELETE FROM sessions WHERE expires < ?1", [now_ms])?;
        Ok(())
    }
}
