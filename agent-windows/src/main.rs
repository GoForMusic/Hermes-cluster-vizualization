//! The Infra Viz agent for Windows. It runs as a Windows container on a Docker Swarm node (or on a Docker machine of its own), measures that node (CPU, memory, the containers
//! running on it) and reports to the hub over gRPC.
//!
//! Kubernetes needs no Windows agent: the Linux agent watches the whole cluster, Windows nodes included, through the API.
//!
//! Settings (environment): `HUB_URL`, `SOURCE_ID`, `SOURCE_NAME` and `TOKEN` (see `hermes_agentkit::Config`; the hub generates them into the
//! Swarm stack file or the Docker compose file), `COLLECTOR` (`swarm`, the default, or `docker` for a machine that is not in a swarm), and
//! `DOCKER_SOCKET`, default `npipe:////./pipe/docker_engine`.

mod windowshost;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use windowshost::WindowsSampler;

/// The release version the image build passes in (`agent-windows-1.2.3` is 1.2.3), or the crate's version marked `-dev` for a local build.
const VERSION: &str = match option_env!("HERMES_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => concat!(env!("CARGO_PKG_VERSION"), "-dev"),
};

/// The Docker engine of a Windows machine.
const PIPE: &str = "npipe:////./pipe/docker_engine";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // The same two collectors as the Linux agent, picked the same way. Windows has no Kubernetes one, so its default is `swarm`.
    let collector = std::env::var("COLLECTOR")
        .ok()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "swarm".into());
    hermes_swarm::run_collector(&collector, VERSION, PIPE, || {
        Box::new(WindowsSampler::default())
    })
    .await
}
