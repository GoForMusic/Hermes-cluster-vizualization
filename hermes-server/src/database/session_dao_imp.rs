//! Login sessions: a separate table and lifecycle (expiry sweep, bulk delete by user) from `IUserDAO`'s `users` table.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::database::IDbContext;

pub trait ISessionDAO: Send + Sync {
    fn create_session(&self, token_hash: &str, user_id: i64, expires_ms: i64) -> Result<()>;
    /// `(user id, expires in ms)`
    fn session(&self, token_hash: &str) -> Result<Option<(i64, i64)>>;
    fn delete_session(&self, token_hash: &str) -> Result<()>;
    fn delete_user_sessions(&self, user_id: i64) -> Result<()>;
    fn delete_expired_sessions(&self, now_ms: i64) -> Result<()>;
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
    fn create_session(&self, token_hash: &str, user_id: i64, expires_ms: i64) -> Result<()> {
        self.ctx.conn().execute(
            "INSERT INTO sessions(token_hash,user_id,expires) VALUES(?1,?2,?3)",
            params![token_hash, user_id, expires_ms],
        )?;
        Ok(())
    }

    fn session(&self, token_hash: &str) -> Result<Option<(i64, i64)>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                "SELECT user_id,expires FROM sessions WHERE token_hash=?1",
                [token_hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
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
