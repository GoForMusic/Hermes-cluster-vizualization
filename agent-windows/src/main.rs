//! The Infra Viz agent for Windows. It runs as a Windows container on a Docker Swarm node, measures that node (CPU, memory, the containers
//! running on it) and reports to the hub over gRPC.
//!
//! Kubernetes needs no Windows agent: the Linux agent watches the whole cluster, Windows nodes included, through the API.
//!
//! Settings (environment): `HUB_URL`, `SOURCE_ID`, `SOURCE_NAME` and `TOKEN` (see `hermes_agentkit::Config`; the hub generates them into the
//! Swarm stack file), and `DOCKER_SOCKET`, default `npipe:////./pipe/docker_engine`.

mod windowshost;

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use windowshost::WindowsSampler;

/// The release version the image build passes in (`agent-windows-1.2.3` is 1.2.3), or the crate's version marked `-dev` for a local build.
const VERSION: &str = match option_env!("HERMES_VERSION") {
    Some(v) if !v.is_empty() => v,
    _ => concat!(env!("CARGO_PKG_VERSION"), "-dev"),
};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    hermes_swarm::run_agent(VERSION, "npipe:////./pipe/docker_engine", || {
        Box::new(WindowsSampler::default())
    })
    .await
}
