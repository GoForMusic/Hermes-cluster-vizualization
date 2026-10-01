//! The registry the agent images come from: one row in the settings table, with the secret encrypted at rest like a source's.

use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use crate::crypto;
use crate::database::{IDbContext, ISettingsDAO, SettingsDAOImp};
use crate::model::RegistryConfig;

const KEY: &str = "registry";

pub trait IRegistryDAO: Send + Sync {
    /// Nothing stored yet gives the empty config (`is_set()` false).
    fn get_registry(&self) -> RegistryConfig;
    fn set_registry(&self, config: &RegistryConfig) -> Result<()>;
}

pub struct RegistryDAOImp {
    ctx: Arc<dyn IDbContext>,
    settings: SettingsDAOImp,
}

impl RegistryDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self {
            settings: SettingsDAOImp::new(ctx.clone()),
            ctx,
        }
    }
}

impl IRegistryDAO for RegistryDAOImp {
    fn get_registry(&self) -> RegistryConfig {
        let Some(raw) = self.settings.get_setting(KEY) else {
            return RegistryConfig::default();
        };
        let mut config: RegistryConfig = serde_json::from_str(&raw).unwrap_or_default();
        match crypto::decrypt(self.ctx.key(), &config.secret) {
            Ok(secret) => config.secret = secret,
            Err(e) => {
                warn!("cannot decrypt the registry secret: {e:#}");
                config.secret.clear();
            }
        }
        config
    }

    fn set_registry(&self, config: &RegistryConfig) -> Result<()> {
        let mut stored = config.clone();
        stored.secret = crypto::encrypt(self.ctx.key(), &config.secret);
        self.settings
            .set_setting(KEY, &serde_json::to_string(&stored)?)
    }
}
