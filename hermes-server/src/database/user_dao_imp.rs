//! Admin accounts.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::database::IDbContext;
use crate::model::User;

pub trait IUserDAO: Send + Sync {
    fn count_users(&self) -> Result<i64>;
    fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
        created: i64,
    ) -> Result<User>;
    fn user_by_name(&self, name: &str) -> Result<Option<User>>;
    fn user_by_id(&self, id: i64) -> Result<Option<User>>;
    fn set_password(&self, id: i64, hash: &str) -> Result<()>;
}

pub struct UserDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl UserDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }

    fn user(&self, where_clause: &str, arg: impl rusqlite::ToSql) -> Result<Option<User>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                &format!(
                    "SELECT id,username,password_hash,role,created FROM users WHERE {where_clause}"
                ),
                [arg],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        password_hash: r.get(2)?,
                        role: r.get(3)?,
                        created: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }
}

impl IUserDAO for UserDAOImp {
    fn count_users(&self) -> Result<i64> {
        Ok(self
            .ctx
            .conn()
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?)
    }

    fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: &str,
        created: i64,
    ) -> Result<User> {
        let conn = self.ctx.conn();
        conn.execute(
            "INSERT INTO users(username,password_hash,role,created) VALUES(?1,?2,?3,?4)",
            params![username, password_hash, role, created],
        )?;
        Ok(User {
            id: conn.last_insert_rowid(),
            username: username.into(),
            password_hash: password_hash.into(),
            role: role.into(),
            created,
        })
    }

    fn user_by_name(&self, name: &str) -> Result<Option<User>> {
        self.user("username=?1", name)
    }

    fn user_by_id(&self, id: i64) -> Result<Option<User>> {
        self.user("id=?1", id)
    }

    fn set_password(&self, id: i64, hash: &str) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE users SET password_hash=?1 WHERE id=?2",
            params![hash, id],
        )?;
        Ok(())
    }
}
