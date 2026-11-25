//! Admin accounts.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, Row, params};

use crate::database::IDbContext;
use crate::model::User;

/// What it takes to create an account.
pub struct NewUser<'a> {
    pub username: &'a str,
    pub password_hash: &'a str,
    pub role: &'a str,
    pub email: &'a str,
    pub break_glass: bool,
    pub created: i64,
    /// The identity provider's id and the account's id there; empty for a local account.
    pub idp: &'a str,
    pub external_id: &'a str,
}

/// What may change about an account afterwards; `None` leaves it as it is.
#[derive(Default)]
pub struct UserPatch {
    pub email: Option<String>,
    pub disabled: Option<bool>,
    pub break_glass: Option<bool>,
}

pub trait IUserDAO: Send + Sync {
    fn count_users(&self) -> Result<i64>;
    fn create_user(&self, new: &NewUser) -> Result<User>;
    fn user_by_name(&self, name: &str) -> Result<Option<User>>;
    fn user_by_id(&self, id: i64) -> Result<Option<User>>;
    /// The account an identity provider knows as `external_id`.
    fn user_by_external(&self, idp: &str, external_id: &str) -> Result<Option<User>>;
    /// The local accounts (no provider) with this email, compared without regard to case.
    fn local_users_by_email(&self, email: &str) -> Result<Vec<User>>;
    /// Makes a local account the one an identity provider vouches for: its password goes.
    fn link_user(&self, id: i64, idp: &str, external_id: &str) -> Result<()>;
    fn list_users(&self) -> Result<Vec<User>>;
    fn update_user(&self, id: i64, patch: &UserPatch) -> Result<()>;
    fn set_password(&self, id: i64, hash: &str) -> Result<()>;
    /// The account, its sessions and its places in groups.
    fn delete_user(&self, id: i64) -> Result<()>;
}

pub struct UserDAOImp {
    ctx: Arc<dyn IDbContext>,
}

const COLUMNS: &str =
    "id,username,password_hash,role,created,email,disabled,break_glass,idp,external_id";

fn read(r: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        role: r.get(3)?,
        created: r.get(4)?,
        email: r.get(5)?,
        disabled: r.get(6)?,
        break_glass: r.get(7)?,
        idp: r.get(8)?,
        external_id: r.get(9)?,
    })
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
                &format!("SELECT {COLUMNS} FROM users WHERE {where_clause}"),
                [arg],
                read,
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

    fn create_user(&self, new: &NewUser) -> Result<User> {
        let conn = self.ctx.conn();
        conn.execute(
            "INSERT INTO users(username,password_hash,role,created,email,break_glass,idp,external_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                new.username,
                new.password_hash,
                new.role,
                new.created,
                new.email,
                new.break_glass,
                new.idp,
                new.external_id
            ],
        )?;
        Ok(User {
            id: conn.last_insert_rowid(),
            username: new.username.into(),
            password_hash: new.password_hash.into(),
            role: new.role.into(),
            created: new.created,
            email: new.email.into(),
            disabled: false,
            break_glass: new.break_glass,
            idp: new.idp.into(),
            external_id: new.external_id.into(),
        })
    }

    fn user_by_name(&self, name: &str) -> Result<Option<User>> {
        self.user("username=?1", name)
    }

    fn user_by_id(&self, id: i64) -> Result<Option<User>> {
        self.user("id=?1", id)
    }

    fn user_by_external(&self, idp: &str, external_id: &str) -> Result<Option<User>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM users WHERE idp=?1 AND external_id=?2 AND idp<>''"),
                params![idp, external_id],
                read,
            )
            .optional()?)
    }

    fn local_users_by_email(&self, email: &str) -> Result<Vec<User>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM users WHERE idp='' AND email<>'' AND lower(email)=lower(?1) ORDER BY id"
        ))?;
        Ok(stmt.query_map([email], read)?.collect::<Result<_, _>>()?)
    }

    fn link_user(&self, id: i64, idp: &str, external_id: &str) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE users SET idp=?1,external_id=?2,password_hash='' WHERE id=?3",
            params![idp, external_id, id],
        )?;
        Ok(())
    }

    fn list_users(&self) -> Result<Vec<User>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM users ORDER BY id"))?;
        Ok(stmt.query_map([], read)?.collect::<Result<_, _>>()?)
    }

    fn update_user(&self, id: i64, patch: &UserPatch) -> Result<()> {
        let conn = self.ctx.conn();
        if let Some(email) = &patch.email {
            conn.execute("UPDATE users SET email=?1 WHERE id=?2", params![email, id])?;
        }
        if let Some(disabled) = patch.disabled {
            conn.execute(
                "UPDATE users SET disabled=?1 WHERE id=?2",
                params![disabled, id],
            )?;
        }
        if let Some(flag) = patch.break_glass {
            conn.execute(
                "UPDATE users SET break_glass=?1 WHERE id=?2",
                params![flag, id],
            )?;
        }
        Ok(())
    }

    fn set_password(&self, id: i64, hash: &str) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE users SET password_hash=?1 WHERE id=?2",
            params![hash, id],
        )?;
        Ok(())
    }

    fn delete_user(&self, id: i64) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
        tx.execute("DELETE FROM group_members WHERE user_id=?1", [id])?;
        tx.execute("DELETE FROM users WHERE id=?1", [id])?;
        Ok(tx.commit()?)
    }
}
