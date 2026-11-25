//! Policies, and which groups give them.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::database::IDbContext;
use crate::model::Policy;

pub trait IPolicyDAO: Send + Sync {
    fn list_policies(&self) -> Result<Vec<Policy>>;
    fn policy(&self, id: i64) -> Result<Option<Policy>>;
    /// Fails when the name is taken.
    fn create_policy(
        &self,
        name: &str,
        description: &str,
        hcl: &str,
        created: i64,
    ) -> Result<Policy>;
    fn update_policy(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        hcl: Option<&str>,
    ) -> Result<()>;
    /// The policy and where it was given.
    fn delete_policy(&self, id: i64) -> Result<()>;
    /// Every `(group id, policy id)` pair.
    fn group_policies(&self) -> Result<Vec<(i64, i64)>>;
    /// Makes exactly these policies the ones the group gives.
    fn set_group_policies(&self, group: i64, policies: &[i64]) -> Result<()>;
    /// `(name, text)` of every policy a user gets: from the groups they are in, and from the system groups (`everyone`).
    fn policies_of_user(&self, user: i64) -> Result<Vec<(String, String)>>;
}

pub struct PolicyDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl PolicyDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

const COLUMNS: &str = "id,name,description,hcl,builtin,created";

fn read(r: &rusqlite::Row) -> rusqlite::Result<Policy> {
    Ok(Policy {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        hcl: r.get(3)?,
        builtin: r.get(4)?,
        created: r.get(5)?,
    })
}

impl IPolicyDAO for PolicyDAOImp {
    fn list_policies(&self) -> Result<Vec<Policy>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM policies ORDER BY builtin DESC, name"
        ))?;
        Ok(stmt.query_map([], read)?.collect::<Result<_, _>>()?)
    }

    fn policy(&self, id: i64) -> Result<Option<Policy>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM policies WHERE id=?1"),
                [id],
                read,
            )
            .optional()?)
    }

    fn create_policy(
        &self,
        name: &str,
        description: &str,
        hcl: &str,
        created: i64,
    ) -> Result<Policy> {
        let conn = self.ctx.conn();
        conn.execute(
            "INSERT INTO policies(name,description,hcl,builtin,created) VALUES(?1,?2,?3,0,?4)",
            params![name, description, hcl, created],
        )?;
        Ok(Policy {
            id: conn.last_insert_rowid(),
            name: name.into(),
            description: description.into(),
            hcl: hcl.into(),
            builtin: false,
            created,
        })
    }

    fn update_policy(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        hcl: Option<&str>,
    ) -> Result<()> {
        let conn = self.ctx.conn();
        if let Some(name) = name {
            conn.execute("UPDATE policies SET name=?1 WHERE id=?2", params![name, id])?;
        }
        if let Some(description) = description {
            conn.execute(
                "UPDATE policies SET description=?1 WHERE id=?2",
                params![description, id],
            )?;
        }
        if let Some(hcl) = hcl {
            conn.execute("UPDATE policies SET hcl=?1 WHERE id=?2", params![hcl, id])?;
        }
        Ok(())
    }

    fn delete_policy(&self, id: i64) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM group_policies WHERE policy_id=?1", [id])?;
        tx.execute("DELETE FROM policies WHERE id=?1", [id])?;
        Ok(tx.commit()?)
    }

    fn group_policies(&self) -> Result<Vec<(i64, i64)>> {
        let conn = self.ctx.conn();
        let mut stmt = conn
            .prepare("SELECT group_id,policy_id FROM group_policies ORDER BY group_id,policy_id")?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?)
    }

    fn set_group_policies(&self, group: i64, policies: &[i64]) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM group_policies WHERE group_id=?1", [group])?;
        for policy in policies {
            tx.execute(
                "INSERT OR IGNORE INTO group_policies(group_id,policy_id) VALUES(?1,?2)",
                params![group, policy],
            )?;
        }
        Ok(tx.commit()?)
    }

    fn policies_of_user(&self, user: i64) -> Result<Vec<(String, String)>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT p.name,p.hcl FROM policies p
               JOIN group_policies gp ON gp.policy_id=p.id
               JOIN groups g ON g.id=gp.group_id
              WHERE g.system=1 OR g.id IN (SELECT group_id FROM group_members WHERE user_id=?1)
              ORDER BY p.name",
        )?;
        Ok(stmt
            .query_map([user], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?)
    }
}
