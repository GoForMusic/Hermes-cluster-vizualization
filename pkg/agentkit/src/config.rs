use std::path::PathBuf;

use anyhow::{Context, Result};

/// Settings come from the environment, so the same image works everywhere:
///
/// | variable      | meaning                                                                                  |
/// |---------------|------------------------------------------------------------------------------------------|
/// | `HUB_URL`     | where the hub is reachable from here, e.g. `https://hub.example.com` (`http://` = no TLS) |
/// | `SOURCE_ID`   | id of the source created in the hub                                                      |
/// | `SOURCE_NAME` | display name of the cluster                                                              |
/// | `TOKEN`       | agent token issued by the hub for this source                                            |
/// | `AGENT_ID`    | this instance; defaults to the host name (pod name in Kubernetes, container id in Swarm) |
/// | `AGENT_HOST`  | id of the host node this agent runs on; the install manifest sets it                     |
/// | `HUB_CA_FILE` | optional PEM file with the CA that signed the hub's certificate (a private CA)           |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub hub_url: String,
    pub source_id: String,
    pub source_name: String,
    pub token: String,
    pub agent_id: String,
    pub host: String,
    pub ca_file: Option<PathBuf>,
    /// What this agent collects (`kubernetes`, `swarm`, `node`), told to the hub in the hello.
    pub collector: String,
    /// Build of the agent, told to the hub in the hello.
    pub version: String,
}

impl Config {
    pub fn from_env(collector: &str, version: &str) -> Result<Self> {
        Self::from_lookup(collector, version, |key| std::env::var(key).ok())
    }

    pub fn from_lookup(
        collector: &str,
        version: &str,
        get: impl Fn(&str) -> Option<String>,
    ) -> Result<Self> {
        let optional = |key: &str| get(key).filter(|v| !v.is_empty());
        let must = |key: &str| {
            optional(key).with_context(|| format!("missing environment variable {key}"))
        };
        Ok(Self {
            hub_url: must("HUB_URL")?.trim_end_matches('/').to_string(),
            source_id: must("SOURCE_ID")?,
            source_name: must("SOURCE_NAME")?,
            token: must("TOKEN")?,
            agent_id: optional("AGENT_ID")
                .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned()),
            host: optional("AGENT_HOST").unwrap_or_default(),
            ca_file: optional("HUB_CA_FILE").map(PathBuf::from),
            collector: collector.to_string(),
            version: version.to_string(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
