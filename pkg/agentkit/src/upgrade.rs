//! Changing the agent's own image when the hub asks. The hub sends a version and nothing else: the agent keeps the registry and the
//! repository of the image it runs, so a hub that is not what it should be cannot send it to another one.

use std::sync::Arc;

use anyhow::{Result, bail};
use hermes_proto::valid_version;

/// How an accepted upgrade went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeOutcome {
    /// The image was changed; the cluster replaces the agents now.
    Started,
    /// Nothing for this agent to do (another agent of the source does it, or it already runs that version): the reason.
    Skipped(String),
}

/// Applies an upgrade to the cluster this agent runs in. An `Err` is a refusal or a failure, with the reason for the admin.
#[tonic::async_trait]
pub trait IUpgrader: Send + Sync {
    async fn upgrade(&self, version: &str) -> Result<UpgradeOutcome>;
}

pub type SharedUpgrader = Arc<dyn IUpgrader>;

/// `registry:5000/acm/agent:1.0.0` and `1.0.1` give `registry:5000/acm/agent:1.0.1`. A Windows tag keeps its OS suffix
/// (`1.0.0-ltsc2022` becomes `1.0.1-ltsc2022`). An image pinned by digest, or a version that is not one, is refused.
pub fn retag(image: &str, version: &str) -> Result<String> {
    if !valid_version(version) {
        bail!("{version:?} is not a version");
    }
    if image.contains('@') {
        bail!("{image} is pinned by digest: change it by hand");
    }
    let (repo, tag) = match image.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo, tag),
        _ => (image, ""),
    };
    let suffix = tag.find("-ltsc").map_or("", |i| &tag[i..]);
    Ok(format!("{repo}:{version}{suffix}"))
}

#[cfg(test)]
#[path = "../tests/unit/upgrade.rs"]
mod tests;
