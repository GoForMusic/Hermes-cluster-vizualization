//! The Infra Viz hub: the backend of the web app and the place the agents report to.
//!
//! Settings (environment):
//!
//! * `HUB_ADDR`                 where to listen, default `:8080`
//! * `HUB_WEB`                  the directory with the web app, default `./web`
//! * `HUB_DATA`                 where the SQLite database lives, default `./data`
//! * `HUB_AGENT_IMAGE`          the Linux agent image the install manifests point at
//! * `HUB_AGENT_IMAGE_WINDOWS`  optional: the Windows agent image (Swarm only)
//! * `HUB_AGENT_PULL_SECRET`    optional: the image pull secret the Kubernetes manifest references
//! * `HUB_SECRET_KEY`           required: 64 hex chars (32 bytes), AES-256 key that encrypts credentials in
//!   SQLite (kubeconfigs, agent tokens) at rest; generate one with `openssl rand -hex 32` and keep it — losing
//!   it makes existing sources' secrets unreadable
//! * `HUB_TLS_CERT`, `HUB_TLS_KEY`  optional, both or neither: PEM cert and private key, so the hub terminates TLS itself
//!   instead of needing a reverse proxy in front of it. REST/SSE and the agents' gRPC still share the one port.

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use hermes_hub::app::{Hub, Settings, parse_addr};
use hermes_hub::database::Repositories;
use tracing::info;
use tracing_subscriber::EnvFilter;

fn env(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Like `env`, but for a setting with no sane default: fails startup with `why` instead of silently running unset.
fn require_env(key: &str, why: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .with_context(|| format!("{key} is required: {why}"))
}

/// Both set, both empty, never one of each — a hub with a cert but no key (or the other way around) is a config mistake,
/// not a plain-HTTP hub, so it should fail to start rather than silently serve without TLS.
fn tls_paths() -> Result<Option<(String, String)>> {
    let (cert, key) = (env("HUB_TLS_CERT", ""), env("HUB_TLS_KEY", ""));
    match (cert.is_empty(), key.is_empty()) {
        (true, true) => Ok(None),
        (false, false) => Ok(Some((cert, key))),
        _ => anyhow::bail!("HUB_TLS_CERT and HUB_TLS_KEY must both be set, or neither"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    // Process-wide, and every TLS client/server in the hub (the registry client, and the hub's own listener below)
    // shares it — installed once, here, before any of them can race to do it themselves.
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("cannot install the TLS crypto provider"))?;

    let addr = parse_addr(&env("HUB_ADDR", ":8080")).context("HUB_ADDR is not a valid address")?;
    let web = env("HUB_WEB", "./web");
    let data = env("HUB_DATA", "./data");
    std::fs::create_dir_all(&data).with_context(|| format!("cannot create {data}"))?;
    let secret_key = require_env(
        "HUB_SECRET_KEY",
        "64 hex chars — generate one with `openssl rand -hex 32`",
    )?;
    let key = hermes_hub::crypto::Key::from_hex(&secret_key)?;
    let db = Repositories::sqlite(&std::path::Path::new(&data).join("hub.db"), key)?;
    let tls = tls_paths()?;

    let hub = Hub::new(
        db,
        Settings {
            web: web.clone().into(),
            agent_image: env("HUB_AGENT_IMAGE", "hermes-agent-linux:dev"),
            agent_image_windows: env("HUB_AGENT_IMAGE_WINDOWS", ""),
            agent_pull_secret: env("HUB_AGENT_PULL_SECRET", ""),
            tls_enabled: tls.is_some(),
        },
    );
    let sources = hub.report_pull_sources();
    hub.spawn_background();

    let service = hub
        .router()
        .into_make_service_with_connect_info::<SocketAddr>();
    match tls {
        Some((cert, key)) => {
            let config = RustlsConfig::from_pem_file(&cert, &key)
                .await
                .with_context(|| format!("cannot load HUB_TLS_CERT={cert} / HUB_TLS_KEY={key}"))?;
            info!(
                "hub {} listening on {addr} with TLS (web={web} data={data}, {sources} sources)",
                hermes_hub::version::VERSION
            );
            let handle = axum_server::Handle::new();
            tokio::spawn({
                let handle = handle.clone();
                async move {
                    shutdown().await;
                    // give in-flight requests and open agent streams a moment, like `axum::serve`'s graceful shutdown below.
                    handle.graceful_shutdown(Some(Duration::from_secs(30)));
                }
            });
            axum_server::bind_rustls(addr, config)
                .handle(handle)
                .serve(service)
                .await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("cannot listen on {addr}"))?;
            info!(
                "hub {} listening on {addr} (web={web} data={data}, {sources} sources)",
                hermes_hub::version::VERSION
            );
            axum::serve(listener, service)
                .with_graceful_shutdown(shutdown())
                .await?;
        }
    }
    Ok(())
}

async fn shutdown() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("cannot listen for SIGTERM");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
