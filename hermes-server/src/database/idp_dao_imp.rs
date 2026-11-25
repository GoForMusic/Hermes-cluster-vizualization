//! The identity providers (single sign-on) and what their groups claim is mapped to.

use std::sync::Arc;

use anyhow::Result;
use rusqlite::{OptionalExtension, Row, params};
use tracing::warn;

use crate::crypto;
use crate::database::IDbContext;
use crate::model::Provider;

pub trait IIdpDAO: Send + Sync {
    fn list_providers(&self) -> Result<Vec<Provider>>;
    fn provider(&self, id: &str) -> Result<Option<Provider>>;
    fn create_provider(&self, p: &Provider) -> Result<()>;
    /// Everything but the id and when it was made.
    fn update_provider(&self, p: &Provider) -> Result<()>;
    /// The provider and its mapping. (The accounts that came from it stay, and cannot log in until it is back.)
    fn delete_provider(&self, id: &str) -> Result<()>;
    /// `(claim value, group)` pairs.
    fn group_map(&self, provider: &str) -> Result<Vec<(String, i64)>>;
    fn set_group_map(&self, provider: &str, map: &[(String, i64)]) -> Result<()>;
}

pub struct IdpDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl IdpDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }

    fn read(&self, r: &Row) -> rusqlite::Result<Provider> {
        let stored: String = r.get(5)?;
        let secret = crypto::decrypt(self.ctx.key(), &stored).unwrap_or_else(|e| {
            warn!("cannot decrypt the secret of an identity provider: {e:#}");
            String::new()
        });
        Ok(Provider {
            id: r.get(0)?,
            name: r.get(1)?,
            issuer: r.get(2)?,
            client_id: r.get(3)?,
            scopes: r.get(4)?,
            client_secret: secret,
            groups_claim: r.get(6)?,
            allowed_domains: r.get(7)?,
            enabled: r.get(8)?,
            auto_create: r.get(9)?,
            link_existing: r.get(10)?,
            trust_email: r.get(11)?,
            enforce: r.get(12)?,
            default_group: r.get(13)?,
            created: r.get(14)?,
        })
    }
}

const COLUMNS: &str = "id,name,issuer,client_id,scopes,secret,groups_claim,allowed_domains,enabled,auto_create,link_existing,trust_email,enforce,default_group,created";

impl IIdpDAO for IdpDAOImp {
    fn list_providers(&self) -> Result<Vec<Provider>> {
        let conn = self.ctx.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM identity_providers ORDER BY created, id"
        ))?;
        Ok(stmt
            .query_map([], |r| self.read(r))?
            .collect::<Result<_, _>>()?)
    }

    fn provider(&self, id: &str) -> Result<Option<Provider>> {
        Ok(self
            .ctx
            .conn()
            .query_row(
                &format!("SELECT {COLUMNS} FROM identity_providers WHERE id=?1"),
                [id],
                |r| self.read(r),
            )
            .optional()?)
    }

    fn create_provider(&self, p: &Provider) -> Result<()> {
        self.ctx.conn().execute(
            &format!("INSERT INTO identity_providers({COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"),
            params![
                p.id,
                p.name,
                p.issuer,
                p.client_id,
                p.scopes,
                crypto::encrypt(self.ctx.key(), &p.client_secret),
                p.groups_claim,
                p.allowed_domains,
                p.enabled,
                p.auto_create,
                p.link_existing,
                p.trust_email,
                p.enforce,
                p.default_group,
                p.created
            ],
        )?;
        Ok(())
    }

    fn update_provider(&self, p: &Provider) -> Result<()> {
        self.ctx.conn().execute(
            "UPDATE identity_providers SET name=?2,issuer=?3,client_id=?4,scopes=?5,secret=?6,groups_claim=?7,allowed_domains=?8,
               enabled=?9,auto_create=?10,link_existing=?11,trust_email=?12,enforce=?13,default_group=?14 WHERE id=?1",
            params![
                p.id,
                p.name,
                p.issuer,
                p.client_id,
                p.scopes,
                crypto::encrypt(self.ctx.key(), &p.client_secret),
                p.groups_claim,
                p.allowed_domains,
                p.enabled,
                p.auto_create,
                p.link_existing,
                p.trust_email,
                p.enforce,
                p.default_group
            ],
        )?;
        Ok(())
    }

    fn delete_provider(&self, id: &str) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM idp_group_map WHERE provider=?1", [id])?;
        tx.execute("DELETE FROM identity_providers WHERE id=?1", [id])?;
        Ok(tx.commit()?)
    }

    fn group_map(&self, provider: &str) -> Result<Vec<(String, i64)>> {
        let conn = self.ctx.conn();
        let mut stmt = conn
            .prepare("SELECT claim,group_id FROM idp_group_map WHERE provider=?1 ORDER BY rowid")?;
        Ok(stmt
            .query_map([provider], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?)
    }

    fn set_group_map(&self, provider: &str, map: &[(String, i64)]) -> Result<()> {
        let mut conn = self.ctx.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM idp_group_map WHERE provider=?1", [provider])?;
        for (claim, group) in map {
            tx.execute(
                "INSERT OR IGNORE INTO idp_group_map(provider,claim,group_id) VALUES(?1,?2,?3)",
                params![provider, claim, group],
            )?;
        }
        Ok(tx.commit()?)
    }
}
