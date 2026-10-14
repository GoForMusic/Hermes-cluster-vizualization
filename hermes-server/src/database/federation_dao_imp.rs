//! Federation settings (the datacenters a master controls, or the master that controls this hub): one row in the settings table, with the
//! tokens encrypted at rest like a source's.

use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use crate::crypto;
use crate::database::{IDbContext, ISettingsDAO, SettingsDAOImp};
use crate::model::FederationConfig;

const KEY: &str = "federation";

pub trait IFederationDAO: Send + Sync {
    /// Nothing stored yet is a hub on its own: no member role, no datacenters.
    fn get_federation(&self) -> FederationConfig;
    fn set_federation(&self, config: &FederationConfig) -> Result<()>;
}

pub struct FederationDAOImp {
    ctx: Arc<dyn IDbContext>,
    settings: SettingsDAOImp,
}

impl FederationDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self {
            settings: SettingsDAOImp::new(ctx.clone()),
            ctx,
        }
    }
}

impl IFederationDAO for FederationDAOImp {
    fn get_federation(&self) -> FederationConfig {
        let Some(raw) = self.settings.get_setting(KEY) else {
            return FederationConfig::default();
        };
        let mut config: FederationConfig = serde_json::from_str(&raw).unwrap_or_default();
        for dc in &mut config.datacenters {
            match crypto::decrypt(self.ctx.key(), &dc.token) {
                Ok(token) => dc.token = token,
                Err(e) => {
                    warn!(datacenter = %dc.name, "cannot decrypt the link token: {e:#}");
                    dc.token.clear();
                }
            }
        }
        config
    }

    fn set_federation(&self, config: &FederationConfig) -> Result<()> {
        let mut stored = config.clone();
        for dc in &mut stored.datacenters {
            dc.token = crypto::encrypt(self.ctx.key(), &dc.token);
        }
        self.settings
            .set_setting(KEY, &serde_json::to_string(&stored)?)
    }
}
