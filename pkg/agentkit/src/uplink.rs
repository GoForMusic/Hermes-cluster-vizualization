//! The agent's `ISink`: collectors write into it, and it ships the changes to the hub over one long-lived gRPC stream.
//! It survives hub restarts and dropped connections by sending its last snapshot again. The queue itself (`State`) is a
//! pure in-memory state machine with no I/O — see `queue.rs`; this file is the session/transport around it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use hermes_proto::PROTOCOL;
use hermes_proto::prefix::{PathPrefix, split_address};
use hermes_proto::v1::agent_service_client::AgentServiceClient;
use hermes_proto::v1::{
    AgentMessage, Batch, CollectorState, Edge, Flow, Hello, LogChunk, LogRequest, Node, Own,
    Upgrade, UpgradeState, UpgradeStatus,
};
use hermes_proto::v1::{agent_message, hub_message};
use prost_types::Struct;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint};
use tracing::{info, warn};

use crate::logs::{LogError, LogQuery, MAX_TAIL};
use crate::queue::State;
use crate::{Config, ISink, SharedLogs, SharedUpgrader, UpgradeOutcome};

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
    logs: Option<SharedLogs>,
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
            logs: None,
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

    /// This agent can read the logs of the containers it reports when the hub asks; the hello says so.
    #[must_use]
    pub fn with_logs(mut self, logs: SharedLogs) -> Self {
        self.logs = Some(logs);
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
        let (_, prefix) = split_address(&self.cfg.hub_url);
        let channel = self
            .endpoint()?
            .connect()
            .await
            .context("cannot reach the hub")?;
        // a hub behind a proxy at a sub-path (`HUB_URL=https://host/hermes`): the path goes in front of every call
        let mut client = AgentServiceClient::new(PathPrefix::new(channel, prefix));

        let (tx, rx) = mpsc::channel::<AgentMessage>(8);
        let hello = Hello {
            protocol: PROTOCOL,
            agent: self.cfg.agent_id.clone(),
            host: self.cfg.host.clone(),
            version: self.cfg.version.clone(),
            collector: self.cfg.collector.clone(),
            features: [
                ("upgrade", self.upgrader.is_some()),
                ("logs", self.logs.is_some()),
            ]
            .into_iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| name.to_string())
            .collect(),
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

        let mut logs_open = LogTasks::default();
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
                        Some(hub_message::Message::LogRequest(r)) => self.log_request(r, &tx, &mut logs_open),
                        Some(hub_message::Message::LogCancel(c)) => logs_open.cancel(c.id),
                        None => {}
                    },
                    None => bail!("the hub closed the stream"),
                },
            }
        }
    }

    /// Starts reading a log the hub asked for. Off the session's loop: a followed log lasts as long as someone watches it.
    fn log_request(&self, r: LogRequest, tx: &mpsc::Sender<AgentMessage>, open: &mut LogTasks) {
        let id = r.id;
        let refuse = |why: &str, not_here: bool| {
            let _ = tx.try_send(log_chunk(LogChunk {
                id,
                error: why.into(),
                not_here,
                ..Default::default()
            }));
        };
        let Some(logs) = self.logs.clone() else {
            return refuse("this agent was not installed to read logs", true);
        };
        open.reap();
        if open.len() >= MAX_LOGS_OPEN {
            return refuse(
                "too many logs are being read through this agent: close one",
                false,
            );
        }
        let query = LogQuery {
            namespace: r.namespace,
            name: r.name,
            container: r.container,
            tail: r.tail.min(MAX_TAIL),
            follow: r.follow,
            previous: r.previous,
            since_seconds: r.since_seconds,
            timestamps: r.timestamps,
            task: r.task,
        };
        let tx = tx.clone();
        open.insert(id, tokio::spawn(stream_log(logs, id, query, tx)));
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
        let (origin, _) = split_address(&self.cfg.hub_url);
        let mut endpoint = Endpoint::from_shared(origin)
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

    fn contribute_with_edges(&self, nodes: Vec<Node>, edges: Vec<Edge>) {
        self.state().contribute_with_edges(nodes, edges);
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

/// How many logs one agent reads at once: each is a connection to the engine or the API, and a stream to the hub.
const MAX_LOGS_OPEN: usize = 4;

fn log_chunk(chunk: LogChunk) -> AgentMessage {
    AgentMessage {
        message: Some(agent_message::Message::LogChunk(chunk)),
    }
}

/// The logs being read for the hub. Whatever is still open when the session ends is stopped: nobody is left to watch it.
#[derive(Default)]
struct LogTasks(HashMap<u64, tokio::task::JoinHandle<()>>);

impl LogTasks {
    fn insert(&mut self, id: u64, task: tokio::task::JoinHandle<()>) {
        if let Some(old) = self.0.insert(id, task) {
            old.abort();
        }
    }

    fn cancel(&mut self, id: u64) {
        if let Some(task) = self.0.remove(&id) {
            task.abort();
        }
    }

    /// Forgets the ones that finished.
    fn reap(&mut self) {
        self.0.retain(|_, t| !t.is_finished());
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

impl Drop for LogTasks {
    fn drop(&mut self) {
        for task in self.0.values() {
            task.abort();
        }
    }
}

/// Reads one log and sends it to the hub as it comes, and then says how it ended.
async fn stream_log(logs: SharedLogs, id: u64, query: LogQuery, tx: mpsc::Sender<AgentMessage>) {
    let (out, mut lines) = mpsc::channel::<Vec<String>>(4);
    let read = logs.read(query, out);
    tokio::pin!(read);
    let mut result: Option<Result<(), LogError>> = None;
    loop {
        tokio::select! {
            Some(batch) = lines.recv() => {
                if tx.send(log_chunk(LogChunk { id, lines: batch, ..Default::default() })).await.is_err() {
                    return;
                }
            }
            r = &mut read, if result.is_none() => result = Some(r),
            else => break,
        }
    }
    let last = match result {
        Some(Err(LogError::NotHere(why))) => LogChunk {
            id,
            error: why,
            not_here: true,
            ..Default::default()
        },
        Some(Err(LogError::Failed(why))) => LogChunk {
            id,
            error: why,
            ..Default::default()
        },
        Some(Ok(())) | None => LogChunk {
            id,
            end: true,
            ..Default::default()
        },
    };
    let _ = tx.send(log_chunk(last)).await;
}
