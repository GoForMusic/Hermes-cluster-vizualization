//! Upgrading the agent from the dashboard, on Docker Swarm: the hub asks for a version and a manager's agent changes the image of the
//! agent's own services (`agent`, and `agent-windows` when the stack has it) to that tag, keeping the registry and repository. Swarm
//! replaces the tasks one at a time, starting the new one before stopping the old, and puts the service back by itself if the new
//! task does not stay up (`failure_action: rollback`).
//!
//! The agent already has the Docker socket, so nothing more is granted; the install manifest only turns `UPGRADES` on when the admin
//! allowed it.

use std::sync::Arc;

use anyhow::{Context, Result};
use hermes_agentkit::{IUpgrader, UpgradeOutcome, retag};
use serde_json::{Value, json};
use tracing::info;

use crate::engine::Engine;

const NAMESPACE_LABEL: &str = "com.docker.stack.namespace";
/// The services of the install stack, by their name without the stack's: the Windows one first, the one this agent runs in last.
const SERVICES: [&str; 2] = ["agent-windows", "agent"];
const SECOND: u64 = 1_000_000_000;

pub struct SwarmUpgrader {
    engine: Arc<Engine>,
}

impl SwarmUpgrader {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }

    /// The stack this agent was deployed with: from the labels of its own container (the host name is the container's id).
    async fn stack(&self) -> Result<String> {
        let id = gethostname::gethostname().to_string_lossy().into_owned();
        let me: Value = self
            .engine
            .get(&format!("/containers/{id}/json"))
            .await
            .context("cannot inspect the agent's own container")?;
        me["Config"]["Labels"][NAMESPACE_LABEL]
            .as_str()
            .map(str::to_string)
            .context("the agent was not deployed as a stack: change its image by hand")
    }
}

/// `repo:tag@sha256:…` is how Swarm stores an image it resolved: only the tag changes, so the digest goes.
fn without_digest(image: &str) -> &str {
    image.split('@').next().unwrap_or(image)
}

#[hermes_agentkit::async_trait]
impl IUpgrader for SwarmUpgrader {
    async fn upgrade(&self, version: &str) -> Result<UpgradeOutcome> {
        let info: Value = self
            .engine
            .get("/info")
            .await
            .context("cannot reach the Docker engine")?;
        if info["Swarm"]["ControlAvailable"] != json!(true) {
            return Ok(UpgradeOutcome::Skipped(
                "a worker: a manager's agent changes the services".into(),
            ));
        }
        let stack = self.stack().await?;
        let mut changed = 0;
        for name in SERVICES {
            let full = format!("{stack}_{name}");
            let found: Value = self
                .engine
                .get(&format!("/services/{full}"))
                .await
                .or_else(|e| {
                    if name == "agent-windows" {
                        Ok(Value::Null)
                    } else {
                        Err(e)
                    }
                })?;
            if found.is_null() {
                continue; // a stack without Windows nodes has no such service
            }
            let mut spec = found["Spec"].clone();
            let current = spec["TaskTemplate"]["ContainerSpec"]["Image"]
                .as_str()
                .context("the service has no image")?
                .to_string();
            let target = retag(without_digest(&current), version)?;
            if without_digest(&current) == target {
                continue;
            }
            info!("upgrade: {full} {current} -> {target}");
            spec["TaskTemplate"]["ContainerSpec"]["Image"] = json!(target);
            spec["UpdateConfig"] = json!({"Parallelism": 1, "Delay": 5 * SECOND, "FailureAction": "rollback", "Monitor": 15 * SECOND, "Order": "start-first"});
            spec["RollbackConfig"] =
                json!({"Parallelism": 1, "FailureAction": "pause", "Order": "start-first"});
            let index = found["Version"]["Index"]
                .as_u64()
                .context("the service has no version")?;
            self.engine
                .post(
                    &format!(
                        "/services/{}/update?version={index}",
                        found["ID"].as_str().unwrap_or(&full)
                    ),
                    &spec,
                )
                .await
                .with_context(|| format!("cannot change the image of {full}"))?;
            changed += 1;
        }
        if changed == 0 {
            return Ok(UpgradeOutcome::Skipped(format!(
                "already running {version}"
            )));
        }
        Ok(UpgradeOutcome::Started)
    }
}

// The fake Docker engine here listens on a real unix socket (unlike engine.rs's tests, this one never exercises the
// Windows named-pipe path) — Windows has no `tokio::net::UnixListener`, so this suite only builds on Unix.
#[cfg(all(test, unix))]
#[path = "../tests/unit/upgrade.rs"]
mod tests;
