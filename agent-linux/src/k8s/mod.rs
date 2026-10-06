//! The Kubernetes collector: it polls the API server every few seconds and turns nodes, pods and PVCs into hosts, workloads and volumes.
//! Metrics come from metrics-server, and volume usage and pod traffic from the kubelet's stats summary; both are best effort.
//!
//! It runs inside the cluster, with the pod's service account.

mod client;
mod kubelet;
mod metrics;
mod network;
mod quantity;
mod state;
mod topology;
mod upgrade;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hermes_agentkit::SharedSink;
use hermes_agentkit::differ::Differ;
use hermes_agentkit::netrate::Tracker;
use hermes_proto::v1::{CollectorState, NodeKind};
use k8s_openapi::api::core::v1::{Namespace, Node, PersistentVolumeClaim, Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::{Ingress, NetworkPolicy};
use kube::api::{ApiResource, DynamicObject, GroupVersionKind, ListParams};
use kube::{Api, Client};
use tracing::debug;

use metrics::Usage;
use topology::Inputs;

const POLL_EVERY: Duration = Duration::from_secs(3);
const STATS_EVERY: Duration = Duration::from_secs(15);
const TIMEOUT: Duration = Duration::from_secs(10);
const GATEWAY_RECHECK: Duration = Duration::from_secs(60);
/// This collector's own meta fields that change while the topology stays the same: a pod's restart count and phase, its container list,
/// and whether a volume's usage has been measured yet.
const RUNTIME_META: &[&str] = &["restarts", "phase", "containers", "usageKnown"];

/// Watches the cluster the pod runs in until the collector fails (the caller restarts it).
pub async fn run(source_id: &str, source_name: &str, sink: SharedSink) -> Result<()> {
    let config = kube::Config::incluster()
        .context("not running inside a Kubernetes cluster (no service account)")?;
    watch(config, source_id, source_name, sink).await
}

/// The upgrader for the cluster this pod runs in: it patches the agent's own Deployment and DaemonSet, in the pod's own namespace.
pub fn upgrader() -> Result<hermes_agentkit::SharedUpgrader> {
    let config = kube::Config::incluster()
        .context("not running inside a Kubernetes cluster (no service account)")?;
    let namespace =
        std::fs::read_to_string("/var/run/secrets/kubernetes.io/serviceaccount/namespace")
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|_| "hermes".into());
    let client = Client::try_from(config).context("cannot set up the Kubernetes client")?;
    Ok(std::sync::Arc::new(upgrade::K8sUpgrader::new(
        client, &namespace,
    )))
}

/// Watches the cluster behind `config` (the tests point it at a fake API server).
async fn watch(
    mut config: kube::Config,
    source_id: &str,
    source_name: &str,
    sink: SharedSink,
) -> Result<()> {
    config.connect_timeout = Some(TIMEOUT);
    config.read_timeout = Some(TIMEOUT);
    config.write_timeout = Some(TIMEOUT);
    let api = config.cluster_url.to_string();
    let client = Client::try_from(config).context("cannot set up the Kubernetes client")?;
    let version = client
        .apiserver_version()
        .await
        .with_context(|| format!("cannot reach {api}"))?
        .git_version;
    let uid = cluster_uid(&client).await;

    let mut poller = Poller {
        client,
        source_id: source_id.to_string(),
        source_name: source_name.to_string(),
        version,
        api,
        uid,
        sink: sink.clone(),
        differ: Differ::new(sink, RUNTIME_META),
        volumes: HashMap::new(),
        stats_at: None,
        gateway_api: None,
        net: Tracker::new(),
        rates: HashMap::new(),
    };
    let mut tick = tokio::time::interval(POLL_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        poller.poll().await?;
    }
}

/// The UID of the `kube-system` namespace: stable for the life of the cluster and unique to it. Empty when it cannot be read, in which case
/// the hub skips its check for the same cluster added twice.
async fn cluster_uid(client: &Client) -> String {
    Api::<Namespace>::all(client.clone())
        .get("kube-system")
        .await
        .ok()
        .and_then(|n| n.metadata.uid)
        .unwrap_or_default()
}

struct Poller {
    client: Client,
    source_id: String,
    source_name: String,
    version: String,
    api: String,
    uid: String,
    sink: SharedSink,
    differ: Differ,
    /// `ns/pvc` -> GiB used, from the last time the kubelets were asked
    volumes: HashMap<String, f64>,
    stats_at: Option<Instant>,
    /// When the Gateway API was last looked for, and whether it is installed: a cluster without its CRDs is not asked again every poll.
    gateway_api: Option<(Instant, bool)>,
    /// pod network counters -> throughput
    net: Tracker,
    /// `ns/pod` -> (received, sent) in Mb/s, from the last read of the kubelets
    rates: HashMap<String, (f64, f64)>,
}

impl Poller {
    /// The Gateway API objects (Gateways and HTTPRoutes), when the cluster has the CRDs. Without them the API answers 404, and the next look is a
    /// minute later.
    async fn gateway_objects(&mut self) -> (Vec<DynamicObject>, Vec<DynamicObject>) {
        if let Some((at, false)) = self.gateway_api
            && at.elapsed() < GATEWAY_RECHECK
        {
            return (Vec::new(), Vec::new());
        }
        let list = |kind: &'static str| {
            let api = Api::<DynamicObject>::all_with(
                self.client.clone(),
                &ApiResource::from_gvk(&GroupVersionKind::gvk(
                    "gateway.networking.k8s.io",
                    "v1",
                    kind,
                )),
            );
            async move { api.list(&ListParams::default()).await }
        };
        let gateways = list("Gateway").await;
        self.gateway_api = Some((Instant::now(), gateways.is_ok()));
        let Ok(gateways) = gateways else {
            return (Vec::new(), Vec::new());
        };
        let routes = list("HTTPRoute").await.map(|l| l.items).unwrap_or_default();
        (gateways.items, routes)
    }

    async fn poll(&mut self) -> Result<()> {
        // Seven independent GETs against the API server, in parallel: one round trip's latency instead of seven, the way the Swarm
        // collector's own poll already does (`hermes_swarm::watch_topology`'s `tokio::try_join!`).
        let (nodes, pods, pvcs, services, slices, ingresses, policies) = tokio::join!(
            async {
                Api::<Node>::all(self.client.clone())
                    .list(&ListParams::default())
                    .await
            },
            client::list::<Pod>(&self.client),
            client::list::<PersistentVolumeClaim>(&self.client),
            client::list_optional::<Service>(&self.client),
            client::list_optional::<EndpointSlice>(&self.client),
            client::list_optional::<Ingress>(&self.client),
            client::list_optional::<NetworkPolicy>(&self.client),
        );
        let nodes: Vec<Node> = nodes.context("cannot list the nodes")?.items;
        let pods: Vec<Pod> = pods.context("cannot list the pods")?;
        let pvcs: Vec<PersistentVolumeClaim> = pvcs.context("cannot list the volume claims")?;
        let (gateways, routes) = self.gateway_objects().await;

        if self.stats_at.is_none_or(|t| t.elapsed() > STATS_EVERY) {
            self.stats_at = Some(Instant::now());
            let sizes: HashMap<String, f64> = pvcs
                .iter()
                .map(|p| {
                    (
                        format!(
                            "{}/{}",
                            p.metadata.namespace.as_deref().unwrap_or_default(),
                            p.metadata.name.as_deref().unwrap_or_default()
                        ),
                        topology::claim_size(p),
                    )
                })
                .collect();
            self.refresh_kubelet_stats(&nodes, &sizes).await;
        }
        let node_usage: HashMap<String, Usage> =
            client::raw(&self.client, "/apis/metrics.k8s.io/v1beta1/nodes")
                .await
                .map(|t| metrics::parse_nodes(&t))
                .unwrap_or_default();
        let pod_usage: HashMap<String, Usage> =
            client::raw(&self.client, "/apis/metrics.k8s.io/v1beta1/pods")
                .await
                .map(|t| metrics::parse_pods(&t))
                .unwrap_or_default();

        let now_ms = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis()),
        )
        .unwrap_or(0);
        let (built, edges) = topology::build(&Inputs {
            source_id: &self.source_id,
            source_name: &self.source_name,
            version: &self.version,
            api: &self.api,
            uid: &self.uid,
            now_ms,
            nodes: &nodes,
            pods: &pods,
            pvcs: &pvcs,
            services: &services,
            slices: &slices,
            ingresses: &ingresses,
            gateways: &gateways,
            routes: &routes,
            policies: &policies,
            node_usage: &node_usage,
            pod_usage: &pod_usage,
            volume_used: &self.volumes,
            pod_rates: &self.rates,
        });
        let (workloads, volumes) = (
            built
                .iter()
                .filter(|n| n.kind() == NodeKind::Workload)
                .count(),
            built
                .iter()
                .filter(|n| n.kind() == NodeKind::Volume)
                .count(),
        );
        if self.differ.apply(built, edges) {
            self.sink.report(
                CollectorState::Connected,
                &format!(
                    "{} nodes · {workloads} pods · {volumes} volumes",
                    nodes.len()
                ),
            );
        }
        Ok(())
    }

    /// Asks every kubelet for its stats summary: volume usage (PVCs) and each pod's network counters. Needs the `nodes/proxy` permission; a
    /// node that does not answer is skipped, and what cannot be measured is left out (unknown), never reported as zero.
    async fn refresh_kubelet_stats(&mut self, nodes: &[Node], sizes: &HashMap<String, f64>) {
        let (mut seen, mut rates) = (HashSet::new(), HashMap::new());
        for n in nodes {
            let Some(name) = n.metadata.name.as_deref() else {
                continue;
            };
            let Some(text) = client::raw(
                &self.client,
                &format!("/api/v1/nodes/{name}/proxy/stats/summary"),
            )
            .await
            else {
                continue;
            };
            match kubelet::parse_summary(&text) {
                Ok(summary) => kubelet::absorb(
                    &summary,
                    sizes,
                    &mut self.volumes,
                    &mut self.net,
                    &mut seen,
                    &mut rates,
                ),
                Err(e) => debug!("{name}: {e:#}"),
            }
        }
        self.net.keep(&seen);
        self.rates = rates;
    }
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_mod.rs"]
mod tests;
