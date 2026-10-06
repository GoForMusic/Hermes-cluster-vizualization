//! Sources: the clusters/agents the hub watches. `secret` (kubeconfig or agent token) is encrypted at rest with the
//! context's key, transparently to every caller.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::params;
use tracing::warn;

use crate::crypto;
use crate::database::IDbContext;
use crate::database::now_ms;
use crate::model::Source;

pub trait ISourceDAO: Send + Sync {
    fn list_sources(&self) -> Result<Vec<Source>>;
    fn insert_source(&self, s: &Source) -> Result<()>;
    fn delete_source(&self, id: &str) -> Result<()>;
    /// Changes only the name: the id and the token stay, so the agents already installed keep working.
    fn rename_source(&self, id: &str, name: &str) -> Result<()>;
    fn set_source_state(&self, id: &str, state: &str, info: &str) -> Result<()>;
}

pub struct SourceDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl SourceDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

impl ISourceDAO for SourceDAOImp {
    fn list_sources(&self) -> Result<Vec<Source>> {
        let mut rows: Vec<Source> = {
            let conn = self.ctx.conn();
            let mut stmt = conn.prepare(
                "SELECT id,name,type,endpoint,auth,state,info,secret FROM sources ORDER BY created",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(Source {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    endpoint: r.get(3)?,
                    auth: r.get(4)?,
                    state: r.get(5)?,
                    info: r.get(6)?,
                    secret: r.get(7)?, // still the encrypted (or, briefly before migrate_secrets, plaintext) form
                    builtin: false,
                })
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // Most callers (rules, dedupe, the admin page) never look at `secret` at all; only `Service::authenticate`
        // does. One row a wrong/rotated `HUB_SECRET_KEY` can't decrypt must not take the whole cluster list down
        // with it (that source just fails to authenticate, same as if its token were simply wrong).
        for s in &mut rows {
            match crypto::decrypt(self.ctx.key(), &s.secret) {
                Ok(secret) => s.secret = secret,
                Err(e) => {
                    warn!(source = %s.id, error = %e, "cannot decrypt this source's secret; treating it as having none");
                    s.secret = String::new();
                }
            }
        }
        Ok(rows)
    }

    fn insert_source(&self, s: &Source) -> Result<()> {
        let secret = crypto::encrypt(self.ctx.key(), &s.secret);
        self.ctx.conn().execute(
            "INSERT INTO sources(id,name,type,endpoint,auth,state,info,secret,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![s.id, s.name, s.kind, s.endpoint, s.auth, s.state, s.info, secret, now_ms()],
        )?;
        Ok(())
    }

    fn delete_source(&self, id: &str) -> Result<()> {
        self.ctx
            .conn()
            .execute("DELETE FROM sources WHERE id=?1", [id])?;
        Ok(())
    }

    fn rename_source(&self, id: &str, name: &str) -> Result<()> {
        self.ctx
            .conn()
            .execute("UPDATE sources SET name=?1 WHERE id=?2", [name, id])?;
        Ok(())
    }

    fn set_source_state(&self, id: &str, state: &str, info: &str) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE sources SET state=?1, info=?2 WHERE id=?3",
            [state, info, id],
        )?;
        Ok(())
    }
}
