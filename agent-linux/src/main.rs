//! The Infra Viz agent for Linux. It runs inside a Kubernetes cluster or on a Docker Swarm node, collects what it can see and
//! reports it to the hub over gRPC.
//!
//! Settings (environment): `HUB_URL`, `SOURCE_ID`, `SOURCE_NAME`, `TOKEN` and the optional ones are described on
//! [`hermes_agentkit::Config`]. On top of them:
//!
//! * `COLLECTOR`      `kubernetes` (default, inside a cluster) | `swarm` | `node` | `flows`
//! * `FLOWS`         node only: `1` also reports who talks to whom (`COLLECTOR=flows` does only that, and is what the node agent used to need a second pod for)
//! * `CRI_SOCKET`     node only: the container runtime socket, so the agent can say which pods run on its node
//! * `CONNTRACK_FILE` flows only: where the kernel's connection table is, default `/proc/net/nf_conntrack`
//! * `UPGRADES`      `1` lets the hub change this agent's image (kubernetes and swarm); the install manifest sets it only when the admin allowed it
//! * `DOCKER_SOCKET`  swarm only: the Docker engine's socket, default `/var/run/docker.sock`

mod cri;
mod flows;
mod k8s;
mod linuxhost;

use anyhow::{Result, bail};
use hermes_agentkit::{Config, SharedSink};
use linuxhost::LinuxSampler;
use tracing_subscriber::EnvFilter;

/// The install manifest sets `UPGRADES=1` only when the admin allowed upgrades from the dashboard.
fn upgrades_allowed() -> bool {
    std::env::var("UPGRADES").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The release version the image build passes in (`agent-linux-1.2.3` is 1.2.3), or the crate's version marked `-dev` for a local build.
const VERSION: &str = match option_env!("HERMES_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => concat!(env!("CARGO_PKG_VERSION"), "-dev"),
};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)] // the agent waits on sockets: two workers, not one per core of the node
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let collector = std::env::var("COLLECTOR")
        .ok()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "kubernetes".into());
    match collector.as_str() {
        "node" => {
            let cfg = Config::from_env("node", VERSION)?;
            // One per node (a DaemonSet). It reads no cluster API: its heartbeat says this node is alive even when the control plane,
            // which is the only other thing that can tell, is not. No cluster permissions needed.
            let socket = std::env::var("CRI_SOCKET").unwrap_or_default(); // optional: with it the agent also says which pods run here
            // optional: `FLOWS=1` (the pod then runs in the node's network namespace) also reports who talks to whom on this node
            let flows = std::env::var("FLOWS").is_ok_and(|v| !v.is_empty() && v != "0");
            let source_id = cfg.source_id.clone();
            hermes_agentkit::run(cfg, move |sink: SharedSink| {
                let (source_id, socket, sink2) = (source_id.clone(), socket.clone(), sink.clone());
                async move {
                    let pods = async {
                        if socket.is_empty() {
                            std::future::pending::<()>().await;
                            Ok(())
                        } else {
                            cri::run(&source_id, &socket, sink).await
                        }
                    };
                    let traffic = async {
                        if flows {
                            flows::run(sink2).await
                        } else {
                            std::future::pending::<()>().await;
                            Ok(())
                        }
                    };
                    tokio::select! { r = pods => r, r = traffic => r }
                }
            })
            .await
        }
        "kubernetes" => {
            let cfg = Config::from_env("kubernetes", VERSION)?;
            let (source_id, source_name) = (cfg.source_id.clone(), cfg.source_name.clone());
            let upgrader = if upgrades_allowed() {
                Some(k8s::upgrader()?)
            } else {
                None
            };
            hermes_agentkit::run_with(cfg, upgrader, move |sink: SharedSink| {
                let (source_id, source_name) = (source_id.clone(), source_name.clone());
                async move { k8s::run(&source_id, &source_name, sink).await }
            })
            .await
        }
        "swarm" => {
            hermes_swarm::run_agent(VERSION, "/var/run/docker.sock", || {
                Box::new(LinuxSampler::default())
            })
            .await
        }
        "flows" => {
            // One per node, in the host network namespace, where the connections of every pod are tracked. Optional: without it the map has
            // the structure, and the numbers that the kubelets give, but not who talks to whom.
            let cfg = Config::from_env("flows", VERSION)?;
            hermes_agentkit::run(
                cfg,
                |sink: SharedSink| async move { flows::run(sink).await },
            )
            .await
        }
        other => bail!("unknown COLLECTOR {other:?} (kubernetes, swarm, node or flows)"),
    }
}
