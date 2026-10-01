//! The agent's `ISink`: collectors write into it, and it ships the changes to the hub over one long-lived gRPC stream.
//! It survives hub restarts and dropped connections by sending its last snapshot again. The queue itself (`State`) is a
//! pure in-memory state machine with no I/O — see `queue.rs`; this file is the session/transport around it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use hermes_proto::PROTOCOL;
use hermes_proto::v1::agent_service_client::AgentServiceClient;
use hermes_proto::v1::{
    AgentMessage, Batch, CollectorState, Edge, Flow, Hello, Node, Own, Upgrade, UpgradeState,
    UpgradeStatus,
};
use hermes_proto::v1::{agent_message, hub_message};
use prost_types::Struct;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};
use tracing::{info, warn};

use crate::queue::State;
use crate::{Config, ISink, SharedUpgrader, UpgradeOutcome};

/// A session that lasted this long was healthy: the next failure starts the back-off over.
const HEALTHY_AFTER: Duration = Duration::from_secs(10);
/// The same complaint is logged at most this often.
const LOG_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How often the queue is sent. An empty batch is a heartbeat, so this is also the heartbeat.
    pub flush: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            flush: Duration::from_secs(1),
            backoff_min: Duration::from_secs(1),
            backoff_max: Duration::from_secs(15),
        }
    }
}

#[derive(Clone)]
pub struct Uplink {
    cfg: Arc<Config>,
    ca: Option<Vec<u8>>,
    timing: Timing,
    state: Arc<Mutex<State>>,
    upgrader: Option<SharedUpgrader>,
}

impl Uplink {
    pub fn new(cfg: &Config) -> Result<Self> {
        let ca = match &cfg.ca_file {
            Some(path) => Some(
                std::fs::read(path)
                    .with_context(|| format!("cannot read HUB_CA_FILE {}", path.display()))?,
            ),
            None => None,
        };
        Ok(Self {
            cfg: Arc::new(cfg.clone()),
            ca,
            timing: Timing::default(),
            state: Arc::default(),
            upgrader: None,
        })
    }

    #[must_use]
    pub fn with_timing(mut self, timing: Timing) -> Self {
        self.timing = timing;
        self
    }

    /// This agent can change its own image when the hub asks; the hello says so.
    #[must_use]
    pub fn with_upgrader(mut self, upgrader: SharedUpgrader) -> Self {
        self.upgrader = Some(upgrader);
        self
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps a session to the hub open until the task is dropped, reconnecting with a growing pause.
    pub async fn run(self) {
        let mut backoff = self.timing.backoff_min;
        let mut last_logged: Option<Instant> = None;
        loop {
            let started = Instant::now();
            let outcome = self.session().await;
            if started.elapsed() >= HEALTHY_AFTER {
                backoff = self.timing.backoff_min;
            }
            if let Err(e) = outcome
                && last_logged.is_none_or(|t| t.elapsed() >= LOG_EVERY)
            {
                warn!("uplink: {e:#} (will retry)");
                last_logged = Some(Instant::now());
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(self.timing.backoff_max);
        }
    }

    async fn session(&self) -> Result<()> {
        let channel = self
            .endpoint()?
            .connect()
            .await
            .context("cannot reach the hub")?;
        let mut client = AgentServiceClient::new(channel);

        let (tx, rx) = mpsc::channel::<AgentMessage>(8);
        let hello = Hello {
            protocol: PROTOCOL,
            agent: self.cfg.agent_id.clone(),
            host: self.cfg.host.clone(),
            version: self.cfg.version.clone(),
            collector: self.cfg.collector.clone(),
            features: if self.upgrader.is_some() {
                vec!["upgrade".into()]
            } else {
                vec![]
            },
        };
        tx.try_send(AgentMessage {
            message: Some(agent_message::Message::Hello(hello)),
        })?;

        let mut request = Request::new(ReceiverStream::new(rx));
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", self.cfg.token)
                .parse()
                .context("the token is not valid in a header")?,
        );
        let mut inbound = client
            .session(request)
            .await
            .context("the hub refused the session")?
            .into_inner();
        info!("uplink: connected to {}", self.cfg.hub_url);

        // whatever the hub knew before is unknown now: it may have restarted, or missed batches while we were cut off
        self.state().resync = true;

        let mut tick = tokio::time::interval(self.timing.flush);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let events = self.state().take_batch();
                    let batch = AgentMessage { message: Some(agent_message::Message::Batch(Batch { events })) };
                    match tx.try_send(batch) {
                        Ok(()) => {}
                        // the connection is not draining: what was dropped is covered by the snapshot we resend
                        Err(TrySendError::Full(_)) => self.state().resync = true,
                        Err(TrySendError::Closed(_)) => bail!("the connection to the hub closed"),
                    }
                }
                msg = inbound.message() => match msg.context("the hub stream failed")? {
                    Some(m) => match m.message {
                        Some(hub_message::Message::Resync(_)) => self.state().resync = true,
                        Some(hub_message::Message::Upgrade(u)) => self.upgrade(u, &tx),
                        None => {}
                    },
                    None => bail!("the hub closed the stream"),
                },
            }
        }
    }

    /// Runs an upgrade the hub asked for and tells it how it went. Off the session's loop: it can take a while, and the heartbeat must go on.
    fn upgrade(&self, asked: Upgrade, tx: &mpsc::Sender<AgentMessage>) {
        let version = asked.version;
        let (state, message) = (
            UpgradeState::Failed,
            "this agent was not installed to upgrade itself",
        );
        let Some(upgrader) = self.upgrader.clone() else {
            let _ = tx.try_send(status(state, &version, message));
            return;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let reply = match upgrader.upgrade(&version).await {
                Ok(UpgradeOutcome::Started) => {
                    info!("upgrade to {version}: started");
                    status(UpgradeState::Started, &version, "")
                }
                Ok(UpgradeOutcome::Skipped(why)) => status(UpgradeState::Skipped, &version, &why),
                Err(e) => {
                    warn!("upgrade to {version}: {e:#}");
                    status(UpgradeState::Failed, &version, &format!("{e:#}"))
                }
            };
            let _ = tx.send(reply).await;
        });
    }

    fn endpoint(&self) -> Result<Endpoint> {
        let mut endpoint = Endpoint::from_shared(self.cfg.hub_url.clone())
            .with_context(|| format!("HUB_URL {} is not a valid address", self.cfg.hub_url))?
            .connect_timeout(Duration::from_secs(10))
            .tcp_nodelay(true)
            // a hub that vanished without closing the connection (power loss, NAT timeout) is noticed within ~25 s
            .http2_keep_alive_interval(Duration::from_secs(15))
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true);
        if self.cfg.hub_url.starts_with("https://") {
            let mut tls = ClientTlsConfig::new().with_webpki_roots();
            if let Some(pem) = &self.ca {
                tls = tls.ca_certificate(Certificate::from_pem(pem));
            }
            endpoint = endpoint.tls_config(tls).context("cannot set up TLS")?;
        }
        Ok(endpoint)
    }
}

impl ISink for Uplink {
    fn set_topology(&self, nodes: Vec<Node>, edges: Vec<Edge>) {
        self.state().set_topology(nodes, edges);
    }

    fn status(&self, id: &str, own: Own, reason: &str) {
        self.state().status(id, own, reason);
    }

    fn meta(&self, id: &str, patch: Struct) {
        self.state().meta(id, patch);
    }

    fn metrics(&self, nodes: HashMap<String, HashMap<String, f64>>, edges: HashMap<String, f64>) {
        self.state().metrics(nodes, edges);
    }

    fn report(&self, state: CollectorState, info: &str) {
        info!("collector: {state:?} — {info}");
        self.state().report(state, info);
    }

    fn alive(&self, ids: Vec<String>) {
        self.state().alive(ids);
    }

    fn contribute(&self, nodes: Vec<Node>) {
        self.state().contribute(nodes);
    }

    fn flows(&self, flows: Vec<Flow>) {
        self.state().flows(flows);
    }
}

fn status(state: UpgradeState, version: &str, message: &str) -> AgentMessage {
    AgentMessage {
        message: Some(agent_message::Message::UpgradeStatus(UpgradeStatus {
            state: state.into(),
            version: version.to_string(),
            message: message.to_string(),
        })),
    }
}
