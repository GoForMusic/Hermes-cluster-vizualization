//! Alerts: `IAlertDAO` is the interface `services::Engine` and the alerts controller depend on; `AlertDAOImp` is the
//! SQLite-backed implementation, written against `IDbContext` rather than any concrete database.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::params;

use crate::database::IDbContext;
use crate::model::{Alert, Severity};

impl rusqlite::types::ToSql for Severity {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        let s = match self {
            Severity::Warn => "warn",
            Severity::Crit => "crit",
        };
        Ok(s.into())
    }
}

impl rusqlite::types::FromSql for Severity {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        match value.as_str()? {
            "warn" => Ok(Severity::Warn),
            "crit" => Ok(Severity::Crit),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown alert severity {other:?}").into(),
            )),
        }
    }
}

pub trait IAlertDAO: Send + Sync {
    /// Stores the alert and fills in its id. `snapshot` is written once, here, and never touched by `update_alert`: it is what the
    /// node looked like at the moment the alert opened, not a running record of its latest state.
    fn insert_alert(&self, a: &mut Alert) -> Result<()>;
    fn update_alert(&self, a: &Alert) -> Result<()>;
    fn list_alerts(&self, limit: i64) -> Result<Vec<Alert>>;
    fn active_alerts(&self) -> Result<Vec<Alert>>;
    fn get_alert(&self, id: i64) -> Result<Option<Alert>>;
    /// Forgets the resolved alerts that ended before `ts` (the ones still open are never dropped); how many went.
    fn purge_resolved_before(&self, ts: i64) -> Result<usize>;
}

pub struct AlertDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl AlertDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }

    fn alerts(&self, where_and_order: &str, args: impl rusqlite::Params) -> Result<Vec<Alert>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(&format!("SELECT id,key,sev,node_id,title,detail,ts,resolved_ts,ack,snapshot,ack_by,ack_ts FROM alerts {where_and_order}"))?;
        let rows = stmt.query_map(args, |r| {
            Ok(Alert {
                id: r.get(0)?,
                key: r.get(1)?,
                sev: r.get(2)?,
                node_id: r.get(3)?,
                title: r.get(4)?,
                detail: r.get(5)?,
                ts: r.get(6)?,
                resolved_ts: r.get(7)?,
                ack: r.get(8)?,
                snapshot: r.get(9)?,
                ack_by: r.get(10)?,
                ack_ts: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

impl IAlertDAO for AlertDAOImp {
    fn insert_alert(&self, a: &mut Alert) -> Result<()> {
        let conn = self.ctx.conn();
        conn.execute(
            "INSERT INTO alerts(key,sev,node_id,title,detail,ts,snapshot) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![a.key, a.sev, a.node_id, a.title, a.detail, a.ts, a.snapshot],
        )?;
        a.id = conn.last_insert_rowid();
        Ok(())
    }

    fn update_alert(&self, a: &Alert) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE alerts SET sev=?1, title=?2, detail=?3, resolved_ts=?4, ack=?5, ack_by=?6, ack_ts=?7 WHERE id=?8",
            params![a.sev, a.title, a.detail, a.resolved_ts, a.ack, a.ack_by, a.ack_ts, a.id],
        )?;
        Ok(())
    }

    fn list_alerts(&self, limit: i64) -> Result<Vec<Alert>> {
        self.alerts("ORDER BY ts DESC LIMIT ?1", [limit])
    }

    fn active_alerts(&self) -> Result<Vec<Alert>> {
        self.alerts("WHERE resolved_ts IS NULL", [])
    }

    fn purge_resolved_before(&self, ts: i64) -> Result<usize> {
        Ok(self.ctx.conn().execute(
            "DELETE FROM alerts WHERE resolved_ts IS NOT NULL AND resolved_ts < ?1",
            [ts],
        )?)
    }

    fn get_alert(&self, id: i64) -> Result<Option<Alert>> {
        Ok(self.alerts("WHERE id=?1", [id])?.into_iter().next())
    }
}
