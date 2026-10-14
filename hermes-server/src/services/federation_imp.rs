//! Federation settings and the rules around them. A hub is on its own (the default), a **member** (managed by a master: it shows a pairing
//! code, then locks and obeys the master that used the code) or a **master** (it has datacenters it controls). `IFederation` is what the
//! controllers and the link depend on; `FederationImp` keeps it in the database.
//!
//! The link between the two is made by the master (the direction a firewall between the zones lets through), so a member cannot reach out to
//! pair: it shows a one-time code and waits for the master to come with it. The first connection consumes the code and the member answers with a
//! token only the master keeps; from then on that token is the only thing that opens the member.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::database::IFederationDAO;
use crate::model::{
    Datacenter, FederationConfig, MEMBER_PAIRED, MEMBER_WAITING, MemberConfig, MemberView,
};

/// How long a pairing code can be used.
pub const CODE_TTL_MS: i64 = 60 * 60 * 1000;
/// A code that was guessed wrong this many times is dead: a new one has to be asked for.
const MAX_WRONG_CODES: u32 = 10;
/// No look-alikes (0/O, 1/I/L): the code is read from one screen and typed on another.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

#[derive(Debug)]
pub enum FedError {
    NotWaiting,
    Locked,
    Expired,
    BadCode,
    Invalid(String),
    Internal(anyhow::Error),
}

impl fmt::Display for FedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWaiting => f.write_str("this hub is not waiting to be managed"),
            Self::Locked => f.write_str(
                "this hub is managed by another one: it can only be released from there",
            ),
            Self::Expired => f.write_str("the pairing code expired: ask the hub for a new one"),
            Self::BadCode => f.write_str("wrong pairing code"),
            Self::Invalid(why) => f.write_str(why),
            Self::Internal(_) => f.write_str("internal error"),
        }
    }
}

impl From<anyhow::Error> for FedError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

pub trait IFederation: Send + Sync {
    fn config(&self) -> FederationConfig;

    // ---- this hub is (going to be) managed by a master --------------------------------------------------------
    /// The pairing step or the lock, for this hub's own browser. `None` for a hub on its own or a master.
    fn member_view(&self, now: i64) -> Option<MemberView>;
    /// Waiting for a master or already managed by one. Either way it keeps no logins of its own.
    fn is_member(&self) -> bool;
    /// Managed by a master: nothing but the master can use this hub.
    fn is_locked(&self) -> bool;
    /// Starts waiting for a master. Returns the code to show, and when it stops working.
    fn enable_member(&self, agent_address: &str, now: i64) -> Result<(String, i64), FedError>;
    /// A fresh code (the old one is gone). Only while waiting.
    fn new_code(&self, now: i64) -> Result<(String, i64), FedError>;
    /// Gives up waiting (no master came). Only while waiting.
    fn disable_member(&self) -> Result<(), FedError>;
    fn set_agent_address(&self, address: &str) -> Result<(), FedError>;
    /// A master came with a code: consumes it and returns the token the master uses from now on.
    fn pair(
        &self,
        code: &str,
        master: &str,
        datacenter: &str,
        now: i64,
    ) -> Result<String, FedError>;
    /// The master says its names again at every connection: a rename there shows here.
    fn note_master(&self, master: &str, datacenter: &str) -> Result<(), FedError>;
    /// Is this the token of the master that paired?
    fn verify_link(&self, token: &str) -> bool;
    /// Back to being on its own (the master removed it, or the operator did it by hand).
    fn release(&self) -> Result<(), FedError>;
    /// Random for the life of the process: proves that a request comes from the link of this process and not from the network.
    fn is_link_secret(&self, presented: &str) -> bool;
    fn link_secret(&self) -> String;

    // ---- this hub controls datacenters ----------------------------------------------------------------------
    fn datacenters(&self) -> Vec<Datacenter>;
    fn add_datacenter(&self, dc: Datacenter) -> Result<(), FedError>;
    fn update_datacenter(
        &self,
        id: &str,
        name: Option<&str>,
        address: Option<&str>,
    ) -> Result<Datacenter, FedError>;
    fn remove_datacenter(&self, id: &str) -> Result<Option<Datacenter>, FedError>;
    /// What this hub is called next to its datacenters.
    fn name(&self) -> String;
    fn set_name(&self, name: &str) -> Result<(), FedError>;
}

pub type SharedFederation = Arc<dyn IFederation>;

pub struct FederationImp {
    dao: Arc<dyn IFederationDAO>,
    /// Read-modify-write of the stored config happens under this, so two requests cannot lose each other's change.
    write: Mutex<()>,
    wrong_codes: Mutex<u32>,
    secret: String,
}

fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn same(a: &str, b: &str) -> bool {
    !a.is_empty() && a.as_bytes().ct_eq(b.as_bytes()).into()
}

fn random_code() -> String {
    let pick = || char::from(CODE_ALPHABET[rand::random_range(0..CODE_ALPHABET.len())]);
    (0..3)
        .map(|_| (0..4).map(|_| pick()).collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Pasted with spaces or in lower case: still the same code.
fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

impl FederationImp {
    pub fn new(dao: Arc<dyn IFederationDAO>) -> Self {
        Self {
            dao,
            write: Mutex::new(()),
            wrong_codes: Mutex::new(0),
            secret: hex::encode(rand::random::<[u8; 32]>()),
        }
    }

    /// Changes the stored config under the write lock.
    fn edit<T>(
        &self,
        f: impl FnOnce(&mut FederationConfig) -> Result<T, FedError>,
    ) -> Result<T, FedError> {
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut config = self.dao.get_federation();
        let out = f(&mut config)?;
        self.dao.set_federation(&config)?;
        Ok(out)
    }

    fn fresh_code(member: &mut MemberConfig, now: i64) -> (String, i64) {
        let code = random_code();
        member.code_hash = hash(&normalize_code(&code));
        member.code_expires = now + CODE_TTL_MS;
        (code, member.code_expires)
    }
}

impl IFederation for FederationImp {
    fn config(&self) -> FederationConfig {
        self.dao.get_federation()
    }

    fn member_view(&self, now: i64) -> Option<MemberView> {
        let m = self.dao.get_federation().member?;
        let paired = m.is_paired();
        Some(MemberView {
            state: m.state.clone(),
            managed_by: paired.then(|| m.master.clone()),
            datacenter: paired.then(|| m.datacenter.clone()),
            agent_address: m.agent_address,
            code_expires: (!paired).then_some(m.code_expires).filter(|e| *e > now),
        })
    }

    fn is_member(&self) -> bool {
        self.dao.get_federation().member.is_some()
    }

    fn is_locked(&self) -> bool {
        self.dao
            .get_federation()
            .member
            .is_some_and(|m| m.is_paired())
    }

    fn enable_member(&self, agent_address: &str, now: i64) -> Result<(String, i64), FedError> {
        self.edit(|c| {
            if !c.datacenters.is_empty() {
                return Err(FedError::Invalid(
                    "this hub controls datacenters: remove them first".into(),
                ));
            }
            if c.member.as_ref().is_some_and(MemberConfig::is_paired) {
                return Err(FedError::Locked);
            }
            let mut member = MemberConfig {
                state: MEMBER_WAITING.into(),
                agent_address: agent_address.trim().trim_end_matches('/').to_string(),
                ..Default::default()
            };
            let out = Self::fresh_code(&mut member, now);
            c.member = Some(member);
            *self
                .wrong_codes
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = 0;
            Ok(out)
        })
    }

    fn new_code(&self, now: i64) -> Result<(String, i64), FedError> {
        self.edit(|c| match c.member.as_mut() {
            Some(m) if !m.is_paired() => {
                *self
                    .wrong_codes
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = 0;
                Ok(Self::fresh_code(m, now))
            }
            Some(_) => Err(FedError::Locked),
            None => Err(FedError::NotWaiting),
        })
    }

    fn disable_member(&self) -> Result<(), FedError> {
        self.edit(|c| match &c.member {
            Some(m) if m.is_paired() => Err(FedError::Locked),
            Some(_) => {
                c.member = None;
                Ok(())
            }
            None => Err(FedError::NotWaiting),
        })
    }

    fn set_agent_address(&self, address: &str) -> Result<(), FedError> {
        let address = address.trim().trim_end_matches('/');
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            return Err(FedError::Invalid(
                "the address starts with http:// or https://".into(),
            ));
        }
        self.edit(|c| match c.member.as_mut() {
            Some(m) => {
                m.agent_address = address.to_string();
                Ok(())
            }
            None => Err(FedError::NotWaiting),
        })
    }

    fn pair(
        &self,
        code: &str,
        master: &str,
        datacenter: &str,
        now: i64,
    ) -> Result<String, FedError> {
        self.edit(|c| {
            let m = match c.member.as_mut() {
                Some(m) if !m.is_paired() => m,
                Some(_) => return Err(FedError::Locked),
                None => return Err(FedError::NotWaiting),
            };
            if m.code_expires <= now {
                return Err(FedError::Expired);
            }
            // too many wrong guesses: nothing matches, not even the right code, until a new one is asked for
            let mut wrong = self
                .wrong_codes
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if *wrong >= MAX_WRONG_CODES || !same(&m.code_hash, &hash(&normalize_code(code))) {
                *wrong += 1;
                return Err(FedError::BadCode);
            }
            drop(wrong);
            let token = hex::encode(rand::random::<[u8; 32]>());
            m.state = MEMBER_PAIRED.into();
            m.token_hash = hash(&token);
            m.code_hash.clear();
            m.code_expires = 0;
            m.master = master.trim().to_string();
            m.datacenter = datacenter.trim().to_string();
            m.paired_at = now;
            Ok(token)
        })
    }

    fn note_master(&self, master: &str, datacenter: &str) -> Result<(), FedError> {
        let (master, datacenter) = (master.trim(), datacenter.trim());
        if master.is_empty() && datacenter.is_empty() {
            return Ok(()); // a master of an earlier version says nothing
        }
        let current = self.dao.get_federation().member;
        if current
            .as_ref()
            .is_none_or(|m| !m.is_paired() || (m.master == master && m.datacenter == datacenter))
        {
            return Ok(());
        }
        self.edit(|c| {
            if let Some(m) = c.member.as_mut().filter(|m| m.is_paired()) {
                m.master = master.to_string();
                m.datacenter = datacenter.to_string();
            }
            Ok(())
        })
    }

    fn verify_link(&self, token: &str) -> bool {
        self.dao
            .get_federation()
            .member
            .is_some_and(|m| m.is_paired() && same(&m.token_hash, &hash(token)))
    }

    fn release(&self) -> Result<(), FedError> {
        self.edit(|c| {
            c.member = None;
            Ok(())
        })
    }

    fn is_link_secret(&self, presented: &str) -> bool {
        same(&self.secret, presented)
    }

    fn link_secret(&self) -> String {
        self.secret.clone()
    }

    fn datacenters(&self) -> Vec<Datacenter> {
        self.dao.get_federation().datacenters
    }

    fn add_datacenter(&self, dc: Datacenter) -> Result<(), FedError> {
        self.edit(|c| {
            if c.member.is_some() {
                return Err(FedError::Invalid(
                    "this hub is managed by another one: it cannot control datacenters".into(),
                ));
            }
            let name = dc.name.trim();
            if name.is_empty() {
                return Err(FedError::Invalid("give the datacenter a name".into()));
            }
            if c.datacenters
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case(name))
                || c.name().eq_ignore_ascii_case(name)
            {
                return Err(FedError::Invalid(
                    "another datacenter is already called that".into(),
                ));
            }
            c.datacenters.push(dc);
            Ok(())
        })
    }

    fn update_datacenter(
        &self,
        id: &str,
        name: Option<&str>,
        address: Option<&str>,
    ) -> Result<Datacenter, FedError> {
        self.edit(|c| {
            if let Some(name) = name.map(str::trim) {
                if name.is_empty() {
                    return Err(FedError::Invalid("give the datacenter a name".into()));
                }
                if c.datacenters
                    .iter()
                    .any(|d| d.id != id && d.name.eq_ignore_ascii_case(name))
                    || c.name().eq_ignore_ascii_case(name)
                {
                    return Err(FedError::Invalid(
                        "another datacenter is already called that".into(),
                    ));
                }
            }
            let dc = c
                .datacenters
                .iter_mut()
                .find(|d| d.id == id)
                .ok_or_else(|| FedError::Invalid("no such datacenter".into()))?;
            if let Some(name) = name {
                dc.name = name.trim().to_string();
            }
            if let Some(address) = address {
                dc.address = address.trim().trim_end_matches('/').to_string();
            }
            Ok(dc.clone())
        })
    }

    fn remove_datacenter(&self, id: &str) -> Result<Option<Datacenter>, FedError> {
        self.edit(|c| {
            let at = c.datacenters.iter().position(|d| d.id == id);
            Ok(at.map(|i| c.datacenters.remove(i)))
        })
    }

    fn name(&self) -> String {
        self.dao.get_federation().name().to_string()
    }

    fn set_name(&self, name: &str) -> Result<(), FedError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(FedError::Invalid("give this hub a name".into()));
        }
        self.edit(|c| {
            if c.datacenters
                .iter()
                .any(|d| d.name.eq_ignore_ascii_case(name))
            {
                return Err(FedError::Invalid(
                    "a datacenter is already called that".into(),
                ));
            }
            c.name = name.to_string();
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/federation.rs"]
mod tests;
