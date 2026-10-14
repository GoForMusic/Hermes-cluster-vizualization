//! The master's side of a federation: one task per datacenter that dials the member, mirrors what it sees into this hub's live view and
//! carries the admin requests of this hub's dashboard to it.
//!
//! The member's sources are shown like the hub's own: their topology goes into the same `IStore` (source ids are random, so two datacenters
//! never collide), so the map, the TV page and the alert rules need no idea that a node lives in another zone. While the link is down the
//! nodes are marked out of date, never dropped and never shown as healthy.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use hermes_proto::FEDERATION_PROTOCOL;
use hermes_proto::v1::federation_service_client::FederationServiceClient;
use hermes_proto::v1::{
    MasterHello, MasterMessage, MemberHello, Release, Request as LinkRequest, master_message,
    member_message,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint};
use tracing::{info, warn};

use crate::model::{Datacenter, DatacenterView, Edge, Node, Source};
use crate::services::{IStore, SharedFederation};

/// A first snapshot of a big cluster is far above tonic's default limit of 4 MiB.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
/// The member sends its sources every few seconds: this long without a word and the link is dead.
const SILENT_AFTER: Duration = Duration::from_secs(30);
/// How long an admin request waits for the member.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTHY_AFTER: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(15);

/// The sources of the datacenters, for the rules that look for sources that cannot be reached.
pub trait IRemoteSources: Send + Sync {
    fn sources(&self) -> Vec<Source>;
}

#[derive(Debug)]
pub enum LinkError {
    NoSuchDatacenter,
    NotConnected,
    Timeout,
    Lost,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NoSuchDatacenter => "no such datacenter",
            Self::NotConnected => "the datacenter is not connected",
            Self::Timeout => "the datacenter did not answer in time",
            Self::Lost => "the link to the datacenter was lost",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Connecting,
    Connected,
    Unreachable,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Unreachable => "unreachable",
        }
    }
}

struct State {
    /// The name in the settings, kept here so listing the sources (once a second, for the rules) does not read the database.
    name: String,
    phase: Phase,
    info: String,
    seen: Option<Instant>,
    version: String,
    agent_address: String,
    /// The member's source list, as its `GET /api/sources` answered last.
    sources: Vec<Value>,
    /// The source ids this datacenter put in the live view, to take away the ones it no longer has.
    known: HashSet<String>,
    tx: Option<mpsc::Sender<MasterMessage>>,
    pending: HashMap<u64, oneshot::Sender<(u16, Vec<u8>)>>,
    next_request: u64,
}

struct Dc {
    state: Mutex<State>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Dc {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct MasterLinks {
    store: Arc<dyn IStore>,
    fed: SharedFederation,
    ca: Option<Vec<u8>>,
    version: String,
    dcs: Mutex<HashMap<String, Arc<Dc>>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TopologyJson {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl MasterLinks {
    /// `ca`: PEM of an extra certificate authority to trust when a member is reached over `https://` (a private one, usually).
    pub fn new(
        store: Arc<dyn IStore>,
        fed: SharedFederation,
        ca: Option<Vec<u8>>,
        version: &str,
    ) -> Self {
        Self {
            store,
            fed,
            ca,
            version: version.to_string(),
            dcs: Mutex::default(),
        }
    }

    fn dc(&self, id: &str) -> Option<Arc<Dc>> {
        self.dcs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Starts the link to every datacenter that is set up. Needs the runtime.
    pub fn start_all(self: &Arc<Self>) {
        for dc in self.fed.datacenters() {
            self.start(&dc.id, &dc.name);
        }
    }

    /// Starts (or keeps) the task that holds the link to one datacenter.
    pub fn start(self: &Arc<Self>, id: &str, name: &str) {
        let dc = {
            let mut dcs = self.dcs.lock().unwrap_or_else(PoisonError::into_inner);
            if dcs.contains_key(id) {
                return;
            }
            let dc = Arc::new(Dc {
                state: Mutex::new(State {
                    name: name.to_string(),
                    phase: Phase::Connecting,
                    info: "connecting".into(),
                    seen: None,
                    version: String::new(),
                    agent_address: String::new(),
                    sources: Vec::new(),
                    known: HashSet::new(),
                    tx: None,
                    pending: HashMap::new(),
                    next_request: 1,
                }),
                task: Mutex::new(None),
            });
            dcs.insert(id.to_string(), dc.clone());
            dc
        };
        self.spawn_supervisor(id, &dc);
    }

    fn spawn_supervisor(self: &Arc<Self>, id: &str, dc: &Arc<Dc>) {
        let (links, id, mine) = (self.clone(), id.to_string(), dc.clone());
        let task = tokio::spawn(async move { links.supervise(&id, &mine).await });
        *dc.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
    }

    /// Dials again, at the address now in the settings. What the datacenter showed stays (out of date) until the new link says otherwise.
    pub fn reconnect(self: &Arc<Self>, id: &str) {
        let Some(dc) = self.dc(id) else {
            let name = self
                .fed
                .datacenters()
                .into_iter()
                .find(|d| d.id == id)
                .map(|d| d.name)
                .unwrap_or_default();
            return self.start(id, &name);
        };
        if let Some(task) = dc
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
        self.lost(&dc, "connecting to the new address");
        self.spawn_supervisor(id, &dc);
    }

    /// Stops the link and takes what the datacenter showed out of the live view. Tells the member first when `release` (it goes back to being a
    /// hub on its own); a member that cannot be reached stays locked until the operator releases it on the machine.
    pub async fn stop(&self, id: &str, release: bool) -> bool {
        let Some(dc) = self.dc(id) else { return true };
        let mut released = !release;
        if release {
            let tx = dc.lock().tx.clone();
            if let Some(tx) = tx {
                let sent = tx
                    .send(MasterMessage {
                        message: Some(master_message::Message::Release(Release {})),
                    })
                    .await
                    .is_ok();
                // the member closes the stream after it let go: wait for that, not for a fixed time
                tokio::time::sleep(Duration::from_millis(500)).await;
                released = sent;
            }
        }
        self.dcs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        if let Some(task) = dc
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
        let known = std::mem::take(&mut dc.lock().known);
        for src in &known {
            self.store.remove_source(src);
        }
        self.store.publish(&json!({"type": "sources"}));
        released
    }

    // ---- what the browser and the rules see ----------------------------------------------------------------------

    pub fn views(&self) -> Vec<DatacenterView> {
        self.fed
            .datacenters()
            .into_iter()
            .map(|cfg| {
                let Some(dc) = self.dc(&cfg.id) else {
                    return view_of(&cfg, Phase::Connecting, "starting", None, "", "", 0);
                };
                let s = dc.lock();
                view_of(
                    &cfg,
                    s.phase,
                    &s.info,
                    s.seen,
                    &s.version,
                    &s.agent_address,
                    s.sources.len(),
                )
            })
            .collect()
    }

    /// The member's sources as the dashboard lists them, each tagged with its datacenter. While the link is down their state is "error":
    /// what the member last said is not what is true now.
    pub fn source_views(&self) -> Vec<Value> {
        let dcs: Vec<(String, Arc<Dc>)> = self
            .dcs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(id, dc)| (id.clone(), dc.clone()))
            .collect();
        let mut out = Vec::new();
        for (id, dc) in dcs {
            let s = dc.lock();
            let down = s.phase != Phase::Connected;
            for v in &s.sources {
                let mut v = v.clone();
                if let Some(o) = v.as_object_mut() {
                    o.insert("dc".into(), json!(id));
                    o.insert("dcName".into(), json!(s.name));
                    if down {
                        o.insert("state".into(), json!("error"));
                        o.insert(
                            "info".into(),
                            json!(format!("datacenter {} is unreachable", s.name)),
                        );
                        o.insert("canUpgrade".into(), json!(false));
                    }
                }
                out.push(v);
            }
        }
        out
    }

    /// The datacenter was renamed in the settings.
    pub fn renamed(&self, id: &str, name: &str) {
        if let Some(dc) = self.dc(id) {
            dc.lock().name = name.to_string();
            self.store.publish(&json!({"type": "sources"}));
        }
    }

    /// Sends an admin request to the member and waits for its answer.
    pub async fn request(
        &self,
        id: &str,
        method: &str,
        path: &str,
        body: Vec<u8>,
    ) -> Result<(u16, Vec<u8>), LinkError> {
        let dc = self.dc(id).ok_or(LinkError::NoSuchDatacenter)?;
        let (rx, tx, request_id) = {
            let mut s = dc.lock();
            let tx = s.tx.clone().ok_or(LinkError::NotConnected)?;
            let request_id = s.next_request;
            s.next_request += 1;
            let (answer, rx) = oneshot::channel();
            s.pending.insert(request_id, answer);
            (rx, tx, request_id)
        };
        let message = MasterMessage {
            message: Some(master_message::Message::Request(LinkRequest {
                id: request_id,
                method: method.to_string(),
                path: path.to_string(),
                body,
            })),
        };
        if tx.send(message).await.is_err() {
            dc.lock().pending.remove(&request_id);
            return Err(LinkError::Lost);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => Err(LinkError::Lost),
            Err(_) => {
                dc.lock().pending.remove(&request_id);
                Err(LinkError::Timeout)
            }
        }
    }

    // ---- pairing -------------------------------------------------------------------------------------------------

    /// First contact with a member: shows it the code it displayed and gets the token the master uses from then on.
    pub async fn pair(
        &self,
        address: &str,
        code: &str,
        name: &str,
    ) -> Result<(String, MemberHello), String> {
        let channel = self.channel(address).await?;
        let (tx, rx) = mpsc::channel::<MasterMessage>(4);
        tx.send(self.hello(name)).await.map_err(|e| e.to_string())?;
        let mut request = Request::new(ReceiverStream::new(rx));
        request.metadata_mut().insert(
            "x-pairing-code",
            MetadataValue::try_from(code.trim()).map_err(|_| "the code has odd characters")?,
        );
        let mut client =
            FederationServiceClient::new(channel).max_decoding_message_size(MAX_MESSAGE);
        let mut inbound = client
            .link(request)
            .await
            .map_err(|s| status_text(&s))?
            .into_inner();
        let (mut token, mut hello) = (None, None);
        while token.is_none() || hello.is_none() {
            let next = tokio::time::timeout(Duration::from_secs(15), inbound.message())
                .await
                .map_err(|_| "the datacenter did not answer".to_string())?
                .map_err(|s| status_text(&s))?;
            match next.and_then(|m| m.message) {
                Some(member_message::Message::Paired(p)) => token = Some(p.token),
                Some(member_message::Message::Hello(h)) => hello = Some(h),
                Some(_) => {}
                None => return Err("the datacenter closed the connection".into()),
            }
        }
        // `unwrap_or_default` cannot happen: the loop ends only with both
        Ok((token.unwrap_or_default(), hello.unwrap_or_default()))
    }

    // ---- the link ------------------------------------------------------------------------------------------------

    fn hello(&self, name: &str) -> MasterMessage {
        MasterMessage {
            message: Some(master_message::Message::Hello(MasterHello {
                protocol: FEDERATION_PROTOCOL,
                datacenter: name.to_string(),
                version: self.version.clone(),
                master: self.fed.name(),
            })),
        }
    }

    async fn channel(&self, address: &str) -> Result<Channel, String> {
        let mut endpoint = Endpoint::from_shared(address.to_string())
            .map_err(|e| format!("not an address: {e}"))?
            .connect_timeout(Duration::from_secs(10))
            .tcp_nodelay(true)
            .http2_keep_alive_interval(Duration::from_secs(15))
            .keep_alive_timeout(Duration::from_secs(10))
            .keep_alive_while_idle(true);
        if address.starts_with("https://") {
            let mut tls = ClientTlsConfig::new().with_webpki_roots();
            if let Some(pem) = &self.ca {
                tls = tls.ca_certificate(Certificate::from_pem(pem));
            }
            endpoint = endpoint.tls_config(tls).map_err(|e| e.to_string())?;
        }
        endpoint
            .connect()
            .await
            .map_err(|e| format!("cannot reach {address}: {e}"))
    }

    /// Keeps the link up for as long as the datacenter is set up: dials, mirrors, and dials again with a back-off when it drops.
    async fn supervise(self: &Arc<Self>, id: &str, dc: &Arc<Dc>) {
        let mut backoff = BACKOFF_MIN;
        loop {
            let Some(cfg) = self.fed.datacenters().into_iter().find(|d| d.id == id) else {
                return; // removed
            };
            let started = Instant::now();
            let why = match self.session(&cfg, dc).await {
                Ok(()) => "the datacenter closed the link".to_string(),
                Err(why) => why,
            };
            self.lost(dc, &why);
            if started.elapsed() >= HEALTHY_AFTER {
                backoff = BACKOFF_MIN;
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_MAX);
        }
    }

    /// The link is down: what the datacenter showed stays, marked out of date.
    fn lost(&self, dc: &Dc, why: &str) {
        let known: Vec<String> = {
            let mut s = dc.lock();
            if s.phase != Phase::Unreachable || s.info != why {
                warn!("datacenter link lost: {why}");
            }
            s.phase = Phase::Unreachable;
            s.info = why.to_string();
            s.tx = None;
            s.pending.clear(); // dropping the senders tells every waiting request
            s.known.iter().cloned().collect()
        };
        for src in &known {
            self.store.set_stale(src, true);
        }
        self.store.publish(&json!({"type": "sources"}));
    }

    async fn session(&self, cfg: &Datacenter, dc: &Arc<Dc>) -> Result<(), String> {
        if cfg.token.is_empty() {
            return Err("no link token: remove this datacenter and add it again".into());
        }
        dc.lock().phase = Phase::Connecting;
        let channel = self.channel(&cfg.address).await?;
        let (tx, rx) = mpsc::channel::<MasterMessage>(32);
        tx.send(self.hello(&cfg.name))
            .await
            .map_err(|e| e.to_string())?;
        let mut request = Request::new(ReceiverStream::new(rx));
        request.metadata_mut().insert(
            "authorization",
            MetadataValue::try_from(format!("Bearer {}", cfg.token))
                .map_err(|_| "the link token has odd characters")?,
        );
        let mut client =
            FederationServiceClient::new(channel).max_decoding_message_size(MAX_MESSAGE);
        let mut inbound = client
            .link(request)
            .await
            .map_err(|s| status_text(&s))?
            .into_inner();
        info!("linked to datacenter {} at {}", cfg.name, cfg.address);
        loop {
            let next = tokio::time::timeout(SILENT_AFTER, inbound.message())
                .await
                .map_err(|_| "the datacenter went silent".to_string())?
                .map_err(|s| status_text(&s))?;
            let Some(message) = next.and_then(|m| m.message) else {
                return Ok(());
            };
            self.on_message(dc, &tx, message);
        }
    }

    fn on_message(
        &self,
        dc: &Dc,
        tx: &mpsc::Sender<MasterMessage>,
        message: member_message::Message,
    ) {
        use member_message::Message;
        dc.lock().seen = Some(Instant::now());
        match message {
            Message::Hello(h) => {
                if h.protocol > FEDERATION_PROTOCOL {
                    warn!(
                        "the datacenter speaks a newer link protocol ({})",
                        h.protocol
                    );
                }
                let mut s = dc.lock();
                s.phase = Phase::Connected;
                s.info = "connected".into();
                s.version = h.version;
                s.agent_address = h.agent_address;
                s.tx = Some(tx.clone());
                drop(s);
                self.store.publish(&json!({"type": "sources"}));
            }
            Message::Topology(t) => {
                let mut seen = HashSet::new();
                for source in t.sources {
                    let Ok(topo) = serde_json::from_slice::<TopologyJson>(&source.json) else {
                        warn!("a datacenter sent a topology I cannot read ({})", source.id);
                        continue;
                    };
                    self.store.set_topology(&source.id, topo.nodes, topo.edges);
                    seen.insert(source.id);
                }
                let gone: Vec<String> = {
                    let mut s = dc.lock();
                    let gone = s.known.difference(&seen).cloned().collect();
                    s.known = seen;
                    gone
                };
                for src in &gone {
                    self.store.remove_source(src);
                }
            }
            Message::Event(e) => {
                if let Ok(event) = serde_json::from_slice::<Value>(&e.json) {
                    self.apply_event(&event);
                }
            }
            Message::Response(r) => {
                let waiting = dc.lock().pending.remove(&r.id);
                if let Some(answer) = waiting {
                    let _ = answer.send((u16::try_from(r.status).unwrap_or(502), r.body));
                }
            }
            Message::Sources(s) => {
                if let Ok(Value::Array(list)) = serde_json::from_slice::<Value>(&s.json) {
                    dc.lock().sources = list;
                    self.store.publish(&json!({"type": "sources"}));
                }
            }
            Message::Paired(_) => {}
        }
    }

    /// A small change the member's own browsers would get: applied here, which also tells this hub's browsers.
    fn apply_event(&self, event: &Value) {
        match event.get("type").and_then(Value::as_str) {
            Some("status") => {
                let field = |k: &str| event.get(k).and_then(Value::as_str).unwrap_or_default();
                self.store
                    .set_status(field("id"), field("own"), field("reason"));
            }
            Some("meta") => {
                if let (Some(id), Some(Value::Object(meta))) =
                    (event.get("id").and_then(Value::as_str), event.get("meta"))
                {
                    self.store.set_meta(id, meta.clone());
                }
            }
            Some("metrics") => {
                let nodes = event
                    .get("nodes")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                let edges = event
                    .get("edges")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                self.store.apply_metrics(&nodes, &edges);
            }
            Some("flows") => self.store.publish(event),
            _ => {}
        }
    }
}

impl IRemoteSources for MasterLinks {
    fn sources(&self) -> Vec<Source> {
        self.source_views()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect()
    }
}

fn view_of(
    cfg: &Datacenter,
    phase: Phase,
    info: &str,
    seen: Option<Instant>,
    version: &str,
    agent_address: &str,
    sources: usize,
) -> DatacenterView {
    DatacenterView {
        id: cfg.id.clone(),
        name: cfg.name.clone(),
        address: cfg.address.clone(),
        state: phase.name().into(),
        info: info.to_string(),
        seen_ago: seen.map(|at| at.elapsed().as_secs()),
        version: version.to_string(),
        agent_address: agent_address.to_string(),
        sources,
    }
}

/// The member's answer to a refused call, in words for the person adding the datacenter.
fn status_text(s: &tonic::Status) -> String {
    match s.code() {
        tonic::Code::Unauthenticated => {
            "the datacenter refused: wrong or expired pairing code, or this hub is not the one it is paired with".into()
        }
        tonic::Code::Unimplemented => {
            "that address answers, but it is not a hub that can be federated (a hub of this version?)".into()
        }
        _ => format!("{}: {}", s.code(), s.message()),
    }
}
