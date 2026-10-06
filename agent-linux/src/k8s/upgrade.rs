//! Upgrading the agent from the dashboard, on Kubernetes: the hub asks for a version and this changes the image of the agent's own
//! Deployment (`hermes-agent`, the cluster reader) and DaemonSet (`hermes-node`, one per node) to that tag, keeping the registry and
//! repository they already have. Kubernetes replaces the pods one at a time; what does not come up healthy is put back.
//!
//! The install manifest gives the service account rights to read and patch exactly these two objects, and only when the admin ticked
//! "allow upgrades" when adding the source.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use hermes_agentkit::{IUpgrader, UpgradeOutcome, retag};
use k8s_openapi::api::apps::v1::{DaemonSet, Deployment};
use k8s_openapi::api::core::v1::PodSpec;
use kube::Client;
use kube::api::{Api, Patch, PatchParams};
use serde_json::json;
use tracing::{info, warn};

const DEPLOYMENT: &str = "hermes-agent";
const DAEMONSET: &str = "hermes-node";
/// The container of each, by name (the strategic merge patch finds it by it).
const DEPLOYMENT_CONTAINER: &str = "agent";
const DAEMONSET_CONTAINER: &str = "node";

pub struct K8sUpgrader {
    client: Client,
    namespace: String,
    /// How long a rollout gets to become healthy before it is put back.
    rollout_timeout: Duration,
    poll: Duration,
}

impl K8sUpgrader {
    pub fn new(client: Client, namespace: &str) -> Self {
        Self {
            client,
            namespace: namespace.to_string(),
            rollout_timeout: Duration::from_secs(180),
            poll: Duration::from_secs(3),
        }
    }

    #[cfg(test)]
    fn quick(mut self) -> Self {
        (self.rollout_timeout, self.poll) = (Duration::from_millis(300), Duration::from_millis(20));
        self
    }

    fn deployments(&self) -> Api<Deployment> {
        Api::namespaced(self.client.clone(), &self.namespace)
    }

    fn daemonsets(&self) -> Api<DaemonSet> {
        Api::namespaced(self.client.clone(), &self.namespace)
    }
}

/// The image of the container `name` in a pod template.
fn image_of(spec: Option<&PodSpec>, name: &str) -> Option<String> {
    spec?
        .containers
        .iter()
        .find(|c| c.name == name)?
        .image
        .clone()
}

fn image_patch(container: &str, image: &str) -> Patch<serde_json::Value> {
    Patch::Strategic(
        json!({"spec": {"template": {"spec": {"containers": [{"name": container, "image": image}]}}}}),
    )
}

#[tonic::async_trait]
impl IUpgrader for K8sUpgrader {
    async fn upgrade(&self, version: &str) -> Result<UpgradeOutcome> {
        let params = PatchParams::default();
        let deployments = self.deployments();
        let me = deployments.get(DEPLOYMENT).await.context("cannot read the agent's own Deployment (was the source installed with upgrades allowed?)")?;
        let current = image_of(
            me.spec.as_ref().and_then(|s| s.template.spec.as_ref()),
            DEPLOYMENT_CONTAINER,
        )
        .context("the agent's Deployment has no agent container")?;
        let target = retag(&current, version)?;

        // the nodes first: if they do not come up, this agent, the one that can say so, is still the old one
        let mut changed = false;
        if let Some(ds) = self
            .daemonsets()
            .get_opt(DAEMONSET)
            .await
            .context("cannot read the node agents' DaemonSet")?
        {
            let now = image_of(
                ds.spec.as_ref().and_then(|s| s.template.spec.as_ref()),
                DAEMONSET_CONTAINER,
            );
            if let Some(now) = now.filter(|now| *now != retag(now, version).unwrap_or_default()) {
                let new = retag(&now, version)?;
                info!("upgrade: {DAEMONSET} {now} -> {new}");
                self.daemonsets()
                    .patch(DAEMONSET, &params, &image_patch(DAEMONSET_CONTAINER, &new))
                    .await
                    .context("cannot change the node agents' image")?;
                changed = true;
                if let Err(e) = self.wait_daemonset().await {
                    warn!("upgrade: {e:#}: putting {now} back");
                    let _ = self
                        .daemonsets()
                        .patch(DAEMONSET, &params, &image_patch(DAEMONSET_CONTAINER, &now))
                        .await;
                    bail!(
                        "the node agents did not come up on {new}, so they were put back on {now}: {e:#}"
                    );
                }
            }
        }

        if current == target {
            return Ok(if changed {
                UpgradeOutcome::Started
            } else {
                UpgradeOutcome::Skipped(format!("already running {version}"))
            });
        }
        info!("upgrade: {DEPLOYMENT} {current} -> {target}");
        deployments
            .patch(
                DEPLOYMENT,
                &params,
                &image_patch(DEPLOYMENT_CONTAINER, &target),
            )
            .await
            .context("cannot change the agent's image")?;
        // this pod goes away when the new one is ready; if that never happens it is still here, and puts the old image back
        let (api, timeout, poll, previous) =
            (self.deployments(), self.rollout_timeout, self.poll, current);
        tokio::spawn(async move {
            if let Err(e) = wait_deployment(&api, timeout, poll).await {
                warn!("upgrade: {e:#}: putting {previous} back");
                let _ = api
                    .patch(
                        DEPLOYMENT,
                        &PatchParams::default(),
                        &image_patch(DEPLOYMENT_CONTAINER, &previous),
                    )
                    .await;
            }
        });
        Ok(UpgradeOutcome::Started)
    }
}

impl K8sUpgrader {
    async fn wait_daemonset(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + self.rollout_timeout;
        loop {
            let ds = self.daemonsets().get(DAEMONSET).await?;
            let generation_seen =
                ds.status.as_ref().and_then(|s| s.observed_generation) >= ds.metadata.generation;
            if let Some(s) = ds.status.as_ref().filter(|_| generation_seen)
                && s.updated_number_scheduled.unwrap_or(0) >= s.desired_number_scheduled
                && s.number_ready >= s.desired_number_scheduled
                && s.number_available.unwrap_or(0) >= s.desired_number_scheduled
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("not ready after {}s", self.rollout_timeout.as_secs());
            }
            tokio::time::sleep(self.poll).await;
        }
    }
}

async fn wait_deployment(api: &Api<Deployment>, timeout: Duration, poll: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let d = api.get(DEPLOYMENT).await?;
        let wanted = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
        let seen = d.status.as_ref().and_then(|s| s.observed_generation) >= d.metadata.generation;
        if let Some(s) = d.status.as_ref().filter(|_| seen)
            && s.updated_replicas.unwrap_or(0) >= wanted
            && s.available_replicas.unwrap_or(0) >= wanted
            && s.replicas.unwrap_or(0) <= wanted
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("the new agent is not ready after {}s", timeout.as_secs());
        }
        tokio::time::sleep(poll).await;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_upgrade.rs"]
mod tests;
