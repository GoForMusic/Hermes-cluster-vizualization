//! The hub as a whole: the shared state, the routes and the background work.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::http::{HeaderValue, header};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tracing::warn;

use crate::controller;
use crate::database::{Repositories, now_ms};
use crate::services::{
    AuthImp, CommLogImp, EngineImp, GuardImp, IAuth, IEngine, IGuard, IIngestService, IStore,
    IngestServiceImp, SharedCommLog, SharedUpgrades, StoreImp, UpgradeServiceImp,
};

pub struct Settings {
    /// The directory with `index.html` and the rest of the web app.
    pub web: PathBuf,
    /// The Linux agent image the generated install manifests point at.
    pub agent_image: String,
    /// Optional: the Windows agent image (Swarm only).
    pub agent_image_windows: String,
    /// Optional: the name of the image pull secret the Kubernetes manifest references.
    pub agent_pull_secret: String,
    /// Whether this process terminates TLS itself (`HUB_TLS_CERT`/`HUB_TLS_KEY` both set). Only used to decide the session
    /// cookie's `Secure` attribute — the listener itself is chosen in `main`, before any of this exists.
    pub tls_enabled: bool,
}

pub struct AppState {
    pub store: Arc<dyn IStore>,
    pub db: Repositories,
    pub auth: Arc<dyn IAuth>,
    pub rules: Arc<dyn IEngine>,
    pub ingest: Arc<dyn IIngestService>,
    pub guard: Arc<dyn IGuard>,
    /// Which agents can change their own image, and how the version changes asked for are going.
    pub upgrades: SharedUpgrades,
    /// What agents and the hub said to each other; off until an admin turns it on.
    pub comm_log: SharedCommLog,
    pub settings: Settings,
    pub registry: crate::registry::RegistryClient,
    /// The version of the agent image the install manifests point at, when its tag is a version: an agent older than this can be updated.
    pub expected_agent: Option<semver::Version>,
    /// The newest agent version the registry held when last asked (at most a minute ago).
    pub latest_in_registry: std::sync::Mutex<Option<(std::time::Instant, Option<semver::Version>)>>,
}

pub struct Hub {
    pub state: Arc<AppState>,
}

impl AppState {
    /// The agent version a source should be on: the newest stable one in the registry when there is one, else the one the hub's image
    /// setting names. An agent below it is `outdated`.
    pub async fn latest_agent(&self) -> Option<semver::Version> {
        let registry = self.db.registry.get_registry();
        if !registry.is_set() || registry.implicit {
            return self.expected_agent.clone();
        }
        let fresh = |slot: &Option<(std::time::Instant, Option<semver::Version>)>| {
            slot.as_ref()
                .filter(|(at, _)| at.elapsed() < Duration::from_secs(60))
                .map(|(_, v)| v.clone())
        };
        if let Some(v) = fresh(
            &self
                .latest_in_registry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ) {
            return v.or_else(|| self.expected_agent.clone());
        }
        let latest = self
            .registry
            .tags(&registry, registry.linux_repo())
            .await
            .ok()
            .and_then(|tags| {
                tags.iter()
                    .filter_map(|t| crate::version::parse(t))
                    .filter(|v| v.pre.is_empty())
                    .max()
            });
        *self
            .latest_in_registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((std::time::Instant::now(), latest.clone()));
        latest.or_else(|| self.expected_agent.clone())
    }
}

impl Hub {
    pub fn new(db: Repositories, settings: Settings) -> Self {
        let store: Arc<dyn IStore> = Arc::new(StoreImp::new());
        let guard: Arc<dyn IGuard> = Arc::new(GuardImp::new(store.clone(), db.sources.clone())); // one cluster, one source
        let state = AppState {
            rules: Arc::new(EngineImp::new(
                store.clone(),
                db.alerts.clone(),
                db.sources.clone(),
                db.beats.clone(),
                db.settings.clone(),
            )),
            ingest: Arc::new(IngestServiceImp::new(
                guard.clone(),
                store.clone(),
                db.sources.clone(),
            )),
            auth: Arc::new(AuthImp::new(
                db.users.clone(),
                db.sessions.clone(),
                db.settings.clone(),
            )),
            store,
            db,
            guard,
            upgrades: Arc::new(UpgradeServiceImp::new()),
            comm_log: Arc::new(CommLogImp::new()),
            expected_agent: crate::version::image_version(&settings.agent_image),
            latest_in_registry: std::sync::Mutex::new(None),
            settings,
            registry: crate::registry::RegistryClient::new(),
        };
        Self {
            state: Arc::new(state),
        }
    }

    /// REST + SSE for the browsers, gRPC for the agents, and the web app, all on one port.
    pub fn router(&self) -> Router {
        let files = ServeDir::new(&self.state.settings.web);
        controller::routes(self.state.clone())
            .merge(controller::agent_routes(
                self.state.ingest.clone(),
                self.state.upgrades.clone(),
                self.state.comm_log.clone(),
            ))
            .fallback_service(files)
            // a handler that sets its own Cache-Control (the event stream) keeps it; everything else must never be cached
            .layer(SetResponseHeaderLayer::if_not_present(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            ))
    }

    /// Sources the hub reads itself (pull) need a collector of their own, and this hub has none yet. Say so, rather than pretend.
    pub fn report_pull_sources(&self) -> usize {
        let sources = self.state.db.sources.list_sources().unwrap_or_default();
        let pull: Vec<_> = sources.iter().filter(|s| !s.is_agent()).collect();
        for s in &pull {
            let info = format!(
                "this hub has no adapter for {} yet; add the cluster with an agent",
                s.kind
            );
            if let Err(e) = self
                .state
                .db
                .sources
                .set_source_state(&s.id, "error", &info)
            {
                warn!("cannot update source {}: {e:#}", s.id);
            }
            self.state.store.ensure_placeholder(s);
        }
        sources.len()
    }

    /// The rules engine, the watch for silent agents and the purge of expired sessions, until the process ends.
    pub fn spawn_background(&self) {
        let rules = self.state.rules.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tick.tick().await;
                rules.evaluate(now_ms());
            }
        });
        let ingest = self.state.ingest.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                ingest.check(std::time::Instant::now());
            }
        });
        let (alerts, settings) = (self.state.db.alerts.clone(), self.state.db.settings.clone());
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                let days = incident_days(settings.get_setting("settings").as_deref());
                let before = now_ms() - days * 86_400_000;
                match alerts.purge_resolved_before(before) {
                    Ok(0) => {}
                    Ok(n) => {
                        tracing::info!("forgot {n} incident(s) resolved more than {days} days ago")
                    }
                    Err(e) => warn!("cannot purge old incidents: {e:#}"),
                }
            }
        });
        let auth = self.state.auth.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                auth.purge_expired();
            }
        });
    }
}

/// How many days of resolved incidents the hub keeps: `incidentDays` in the settings the admin saved, 30 when there is none.
pub fn incident_days(settings: Option<&str>) -> i64 {
    settings
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get("incidentDays").and_then(serde_json::Value::as_i64))
        .map_or(30, |d| d.clamp(1, 3650))
}

/// `:8765` (all interfaces, as the Go hub read it), `127.0.0.1:8765` or `[::]:8765`.
pub fn parse_addr(addr: &str) -> Result<SocketAddr, std::net::AddrParseError> {
    match addr.strip_prefix(':') {
        Some(port) => format!("0.0.0.0:{port}").parse(),
        None => addr.parse(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/app.rs"]
mod tests;
