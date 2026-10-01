use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hermes_proto::v1::CollectorState;
use tracing::{info, warn};

use crate::{Config, SharedSink, SharedUpgrader, Uplink};

const RESTART_AFTER: Duration = Duration::from_secs(10);

/// Reports to the hub until the process is asked to stop. When the collector fails it is started again after ten seconds.
pub async fn run<F, Fut>(cfg: Config, collect: F) -> Result<()>
where
    F: Fn(SharedSink) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    run_with(cfg, None, collect).await
}

/// [`run`] for an agent that can also change its own image when the hub asks (`upgrader`); the hub is told it can.
pub async fn run_with<F, Fut>(
    cfg: Config,
    upgrader: Option<SharedUpgrader>,
    collect: F,
) -> Result<()>
where
    F: Fn(SharedSink) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let mut uplink = Uplink::new(&cfg)?;
    if let Some(upgrader) = upgrader {
        uplink = uplink.with_upgrader(upgrader);
    }
    let link = tokio::spawn(uplink.clone().run());
    info!(source = %cfg.source_id, name = %cfg.source_name, collector = %cfg.collector, version = %cfg.version, hub = %cfg.hub_url, "agent started");

    let sink: SharedSink = Arc::new(uplink);
    let supervise = async {
        loop {
            let failure = match collect(sink.clone()).await {
                Ok(()) => "the collector stopped".to_string(),
                Err(e) => format!("{e:#}"),
            };
            sink.report(CollectorState::Error, &failure);
            warn!("collector stopped: {failure} (retrying)");
            tokio::time::sleep(RESTART_AFTER).await;
        }
    };
    tokio::select! {
        () = supervise => {}
        () = shutdown_signal() => info!("shutting down"),
    }
    link.abort();
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("cannot listen for SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
