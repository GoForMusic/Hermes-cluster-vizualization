//! Groups of users, and who is in them.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use crate::database::IDbContext;
use crate::model::Group;

pub trait IGroupDAO: Send + Sync {
    fn list_groups(&self) -> Result<Vec<Group>>;
    fn group(&self, id: i64) -> Result<Option<Group>>;
    fn group_by_name(&self, name: &str) -> Result<Option<Group>>;
    /// Fails when the name is taken.
    fn create_group(&self, name: &str, description: &str, created: i64) -> Result<Group>;
    fn update_group(&self, id: i64, name: Option<&str>, description: Option<&str>) -> Result<()>;
    /// The group and its memberships.
    fn delete_group(&self, id: i64) -> Result<()>;
    /// Every `(group id, user id)` pair.
    fn memberships(&self) -> Result<Vec<(i64, i64)>>;
    /// Makes exactly these users the members of the group.
    fn set_members(&self, group: i64, users: &[i64]) -> Result<()>;
}

pub struct GroupDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl GroupDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

fn read(r: &rusqlite::Row) -> rusqlite::Result<Group> {
    Ok(Group {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        created: r.get(3)?,
        system: r.get(4)?,
    })
}

impl IGroupDAO for GroupDAOImp {
    fn list_groups(&self) -> Result<Vec<Group>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(
            "SELECT id,name,description,created,system FROM groups ORDER BY system DESC, name",
        )?;
        Ok(stmt.query_map([], read)?.collect::<Result<_, _>>()?)
    }

    fn group(&self, id: i64) -> Result<Option<Group>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                "SELECT id,name,description,created,system FROM groups WHERE id=?1",
                [id],
                read,
            )
            .optional()?)
    }

    fn group_by_name(&self, name: &str) -> Result<Option<Group>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                "SELECT id,name,description,created,system FROM groups WHERE name=?1",
                [name],
                read,
            )
            .optional()?)
    }

    fn create_group(&self, name: &str, description: &str, created: i64) -> Result<Group> {
        let conn = self.ctx.conn();
        conn.execute(
            "INSERT INTO groups(name,description,created) VALUES(?1,?2,?3)",
            params![name, description, created],
        )?;
        Ok(Group {
            id: conn.last_insert_rowid(),
            name: name.into(),
            description: description.into(),
            created,
            system: false,
        })
    }

    fn update_group(&self, id: i64, name: Option<&str>, description: Option<&str>) -> Result<()> {
        let conn = self.ctx.conn();
        if let Some(name) = name {
            conn.execute("UPDATE groups SET name=?1 WHERE id=?2", params![name, id])?;
        }
        if let Some(description) = description {
            conn.execute(
                "UPDATE groups SET description=?1 WHERE id=?2",
                params![description, id],
            )?;
        }
        Ok(())
    }

    fn delete_group(&self, id: i64) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM group_members WHERE group_id=?1", [id])?;
        tx.execute("DELETE FROM group_policies WHERE group_id=?1", [id])?;
        tx.execute("DELETE FROM groups WHERE id=?1", [id])?;
        Ok(tx.commit()?)
    }

    fn memberships(&self) -> Result<Vec<(i64, i64)>> {
        let conn = self.ctx.conn();
        let mut stmt =
            conn.prepare("SELECT group_id,user_id FROM group_members ORDER BY group_id,user_id")?;
        Ok(stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?)
    }

    fn set_members(&self, group: i64, users: &[i64]) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM group_members WHERE group_id=?1", [group])?;
        for user in users {
            tx.execute(
                "INSERT OR IGNORE INTO group_members(group_id,user_id) VALUES(?1,?2)",
                params![group, user],
            )?;
        }
        Ok(tx.commit()?)
    }
}
