//! Credentials at rest (`sources.secret`: kubeconfigs and agent tokens) are encrypted with a key the operator supplies
//! (`HUB_SECRET_KEY`), so a copy of `hub.db` alone is not enough to read them. AES-256-GCM, one random nonce per value,
//! tagged `enc1:` so old plaintext rows (from before this existed, or from the Go hub) are told apart from new ones.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

const PREFIX: &str = "enc1:";
const NONCE_LEN: usize = 12;

/// A 256-bit AES-GCM key, read once at startup.
pub struct Key(Aes256Gcm);

impl Key {
    /// From `HUB_SECRET_KEY`: 64 hex characters (32 bytes).
    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s.trim()).context("HUB_SECRET_KEY is not valid hex")?;
        let bytes: [u8; 32] = bytes.try_into().map_err(|b: Vec<u8>| {
            anyhow!(
                "HUB_SECRET_KEY must be 32 bytes (64 hex chars), got {}",
                b.len()
            )
        })?;
        Ok(Self(Aes256Gcm::new(&bytes.into())))
    }

    /// A fresh random key, for tests and `Db::open_in_memory` (the database never outlives the process, so there is
    /// nothing to keep the key compatible with).
    pub fn random() -> Self {
        Self(Aes256Gcm::new(&rand::random::<[u8; 32]>().into()))
    }
}

/// Encrypts `plaintext`. An empty string stays empty (nothing to protect, and it lets sources without a secret yet
/// round-trip without ceremony).
pub fn encrypt(key: &Key, plaintext: &str) -> String {
    if plaintext.is_empty() {
        return String::new();
    }
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let nonce = Nonce::try_from(nonce_bytes.as_slice())
        .expect("rand::random() filled exactly NONCE_LEN bytes");
    // The key comes from `HUB_SECRET_KEY`: encryption only fails if that key is malformed, which `Key::from_hex` already rejects.
    let ciphertext = key
        .0
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("AES-GCM encryption with a valid key cannot fail");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    format!("{PREFIX}{}", BASE64.encode(out))
}

/// Decrypts a value produced by [`encrypt`]. Fails on a wrong key, a tampered value, or anything not in the `enc1:`
/// form — callers that might still see old plaintext (the one-time migration in `Db::init`) check for the prefix
/// themselves before calling this.
pub fn decrypt(key: &Key, stored: &str) -> Result<String> {
    if stored.is_empty() {
        return Ok(String::new());
    }
    let Some(b64) = stored.strip_prefix(PREFIX) else {
        bail!("secret is not in the expected enc1: form");
    };
    let raw = BASE64.decode(b64).context("secret is not valid base64")?;
    if raw.len() < NONCE_LEN {
        bail!("secret is too short to hold a nonce");
    }
    let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
    let nonce = Nonce::try_from(nonce_bytes).expect("split_at(NONCE_LEN) guarantees this length");
    let plaintext = key.0.decrypt(&nonce, ciphertext).map_err(|_| {
        anyhow!(
            "secret does not decrypt with this key (wrong HUB_SECRET_KEY, or it was tampered with)"
        )
    })?;
    String::from_utf8(plaintext).context("decrypted secret is not valid UTF-8")
}

/// Whether `stored` still needs the one-time migration to encrypted form.
pub fn is_plaintext(stored: &str) -> bool {
    !stored.is_empty() && !stored.starts_with(PREFIX)
}

#[cfg(test)]
#[path = "../../tests/unit/crypto.rs"]
mod tests;
