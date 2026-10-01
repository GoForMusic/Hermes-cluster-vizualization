//! The hub's own settings blob (the alert rules, TV display options, ...) and small flags stored the same way (the
//! public-view toggle uses `PUBLIC_VIEW_KEY` from `auth`).

use std::sync::Arc;

use anyhow::Result;
use rusqlite::OptionalExtension;

use crate::database::IDbContext;

pub trait ISettingsDAO: Send + Sync {
    fn get_setting(&self, key: &str) -> Option<String>;
    fn set_setting(&self, key: &str, value: &str) -> Result<()>;
}

pub struct SettingsDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl SettingsDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

impl ISettingsDAO for SettingsDAOImp {
    fn get_setting(&self, key: &str) -> Option<String> {
        self.ctx
            .conn()
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten()
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.ctx.conn().execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
        Ok(())
    }
}
