//! The member's side of the link: the gRPC stream a master opens to this hub. The master asks for the hub's admin API (a few paths only)
//! and gets, unasked, everything this hub sees: its sources, topologies and live changes.

use std::pin::Pin;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request as HttpRequest, header};
use hermes_proto::FEDERATION_PROTOCOL;
use hermes_proto::v1::federation_service_server::{FederationService, FederationServiceServer};
use hermes_proto::v1::{
    LiveEvent, LogEntry, MasterMessage, MemberHello, MemberMessage, Paired,
    Response as LinkResponse, SourceTopology, Sources, Topology, master_message, member_message,
};
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc, watch};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use tower::ServiceExt;
use tracing::{info, warn};

use super::{LINK_HEADER, Shared};
use crate::database::now_ms;
use crate::services::{FedError, LogItem};

/// A first snapshot of a big cluster is far above tonic's default limit of 4 MiB.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
/// The master's hello comes right after it connects.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// The topology is sent at most this often, however often the agents change it.
const FLUSH_EVERY: Duration = Duration::from_millis(250);
/// The sources (with their agents) are sent this often, whether they changed or not: it is also what tells the master this hub is alive.
const SOURCES_EVERY: Duration = Duration::from_secs(5);
const BODY_LIMIT: usize = 32 * 1024 * 1024;

/// The admin calls a master may make on a member: managing its sources and registry, and where its agents find it (its agents' traffic log comes up the link by itself while the master's log is on). Nothing else of the
/// admin API (no logins, no settings, and not the release, which has its own message).
pub(super) fn link_allowed(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or_default();
    if path.contains("..") || path.contains("//") {
        return false;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method, segments.as_slice()) {
        ("GET" | "POST", ["api", "sources"])
        | ("GET" | "PUT" | "DELETE", ["api", "registry"])
        | ("POST", ["api", "registry", "test"])
        | ("GET", ["api", "registry", "versions"])
        | ("PATCH", ["api", "federation", "member"]) => true,
        ("PATCH" | "DELETE", ["api", "sources", id]) => !id.is_empty(),
        ("POST", ["api", "sources", id, "upgrade"]) => !id.is_empty(),
        _ => false,
    }
}

/// Adds the link service to the gRPC routes the agents' service is already on: one router can have only one fallback, so both live in one.
pub(super) fn add(routes: tonic::service::Routes, state: Shared) -> tonic::service::Routes {
    let (generation, _) = watch::channel(0_u64);
    let service = FederationServiceServer::new(Members { state, generation })
        .max_decoding_message_size(MAX_MESSAGE);
    routes.add_service(service)
}

struct Members {
    state: Shared,
    /// Counts the links opened to this hub. A new one replaces the one before it: a hub has one master.
    generation: watch::Sender<u64>,
}

type Out = Pin<Box<dyn Stream<Item = Result<MemberMessage, Status>> + Send>>;

fn bearer(req: &Request<Streaming<MasterMessage>>) -> &str {
    req.metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
}

fn refuse(e: &FedError) -> Status {
    match e {
        FedError::NotWaiting | FedError::Locked => Status::failed_precondition(e.to_string()),
        FedError::Expired | FedError::BadCode => Status::unauthenticated(e.to_string()),
        FedError::Invalid(_) => Status::invalid_argument(e.to_string()),
        FedError::Internal(_) => Status::internal("internal error"),
    }
}

#[tonic::async_trait]
impl FederationService for Members {
    type LinkStream = Out;

    async fn link(&self, req: Request<Streaming<MasterMessage>>) -> Result<Response<Out>, Status> {
        let fed = self.state.federation.clone();
        let code = req
            .metadata()
            .get("x-pairing-code")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let token = bearer(&req).to_string();
        // Not a member at all, or a token that is not the master's: the same answer, nothing to learn from it.
        if code.is_none() && !fed.verify_link(&token) {
            return Err(Status::unauthenticated("unknown link token"));
        }
        let mut inbound = req.into_inner();
        let hello = tokio::time::timeout(HELLO_WITHIN, inbound.message())
            .await
            .map_err(|_| Status::deadline_exceeded("the first message must be a hello"))??
            .and_then(|m| m.message);
        let Some(master_message::Message::Hello(hello)) = hello else {
            return Err(Status::invalid_argument(
                "the first message must be a hello",
            ));
        };
        let paired = match code {
            Some(code) => Some(
                fed.pair(&code, &hello.master, &hello.datacenter, now_ms())
                    .map_err(|e| refuse(&e))?,
            ),
            None => {
                let _ = fed.note_master(&hello.master, &hello.datacenter);
                None
            }
        };
        info!(
            "master {} (this datacenter: {}) linked to this hub{} (version {}, protocol {})",
            hello.master,
            hello.datacenter,
            if paired.is_some() { ", paired" } else { "" },
            hello.version,
            hello.protocol
        );

        let (tx, rx) = mpsc::channel(64);
        self.generation.send_modify(|g| *g += 1);
        let session = Session {
            state: self.state.clone(),
            router: super::routes(self.state.clone()),
            tx,
            generation: self.generation.subscribe(),
            mine: *self.generation.borrow(),
        };
        tokio::spawn(session.run(inbound, paired));
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

struct Session {
    state: Shared,
    /// This hub's own admin API, which the master's requests are made on.
    router: Router,
    tx: mpsc::Sender<Result<MemberMessage, Status>>,
    generation: watch::Receiver<u64>,
    mine: u64,
}

fn message(m: member_message::Message) -> Result<MemberMessage, Status> {
    Ok(MemberMessage { message: Some(m) })
}

impl Session {
    async fn send(&self, m: member_message::Message) -> bool {
        self.tx.send(message(m)).await.is_ok()
    }

    /// A call on this hub's own admin API, as the master's admin.
    async fn call(&self, method: &str, path: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
        call(&self.router, &self.state, method, path, body).await
    }

    fn hello(&self) -> member_message::Message {
        let agent_address = self
            .state
            .federation
            .config()
            .member
            .map(|m| m.agent_address)
            .unwrap_or_default();
        member_message::Message::Hello(MemberHello {
            protocol: FEDERATION_PROTOCOL,
            version: crate::version::VERSION.to_string(),
            agent_address,
            features: Vec::new(),
        })
    }

    async fn sources(&self) -> member_message::Message {
        let (_, json) = self.call("GET", "/api/sources", Vec::new()).await;
        member_message::Message::Sources(Sources { json })
    }

    fn topology(&self) -> member_message::Message {
        let mut sources = Vec::new();
        for s in self.state.db.sources.list_sources().unwrap_or_default() {
            if !self.state.store.has_source(&s.id) {
                continue;
            }
            let json = serde_json::to_vec(&json!({
                "nodes": self.state.store.nodes_for(&s.id),
                "edges": self.state.store.edges_for(&s.id),
            }))
            .unwrap_or_default();
            sources.push(SourceTopology { id: s.id, json });
        }
        member_message::Message::Topology(Topology { sources })
    }

    async fn run(self, mut inbound: Streaming<MasterMessage>, paired: Option<String>) {
        // subscribe before the first snapshot is read: nothing is missed in between
        let mut events = self.state.store.subscribe();
        if let Some(token) = paired
            && !self
                .send(member_message::Message::Paired(Paired { token }))
                .await
        {
            return;
        }
        let first = [self.hello(), self.sources().await, self.topology()];
        for m in first {
            if !self.send(m).await {
                return;
            }
        }
        let (mut topology_dirty, mut sources_dirty) = (false, false);
        // while the master's log is on, this hub's log is on too and passed up: what its agents say, with the content
        let mut log_rx: Option<broadcast::Receiver<std::sync::Arc<LogItem>>> = None;
        let mut heartbeats = 0_u64;
        let mut flush = tokio::time::interval(FLUSH_EVERY);
        let mut last_sources = tokio::time::Instant::now();
        loop {
            tokio::select! {
                incoming = inbound.message() => {
                    let Ok(Some(MasterMessage { message: Some(m) })) = incoming else { return };
                    match m {
                        master_message::Message::Request(r) => self.spawn_request(r),
                        master_message::Message::Release(_) => {
                            info!("the master released this hub: it is on its own again");
                            if let Err(e) = self.state.federation.release() {
                                warn!("cannot release this hub: {e}");
                            }
                            return;
                        }
                        master_message::Message::SetLog(l) => {
                            self.state.comm_log.set_enabled(l.enabled);
                            log_rx = l.enabled.then(|| self.state.comm_log.subscribe());
                            heartbeats = 0;
                        }
                        master_message::Message::Hello(_) => {}
                    }
                }
                event = events.recv() => match event {
                    Ok(text) => {
                        if text.starts_with("{\"type\":\"snapshot\"") {
                            topology_dirty = true;
                        } else if let Ok(v) = serde_json::from_str::<Value>(&text) {
                            match v.get("type").and_then(Value::as_str) {
                                Some("sources") => sources_dirty = true,
                                Some("status" | "meta" | "metrics" | "flows") => {
                                    // a lost sample is replaced by the next; the full topology is what must not be lost
                                    let _ = self.tx.try_send(message(member_message::Message::Event(
                                        LiveEvent { json: text.as_bytes().to_vec() },
                                    )));
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        topology_dirty = true;
                        sources_dirty = true;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                item = async {
                    match log_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match item {
                    Ok(item) => match &*item {
                        LogItem::Heartbeat => heartbeats += 1,
                        LogItem::Entry(e) => {
                            // a lost line is not worth holding the link up for
                            let _ = self.tx.try_send(message(member_message::Message::Log(LogEntry {
                                ts: e.ts,
                                source: e.source.clone(),
                                source_name: e.source_name.clone(),
                                agent: e.agent.clone(),
                                dir: e.dir.to_string(),
                                kind: e.kind.to_string(),
                                summary: e.summary.clone(),
                                bytes: e.bytes,
                                body: serde_json::to_vec(&e.body).unwrap_or_default(),
                                heartbeats: 0,
                            })));
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => log_rx = None,
                },
                _ = flush.tick() => {
                    if *self.generation.borrow() != self.mine {
                        return; // another link replaced this one
                    }
                    if heartbeats > 0 {
                        let _ = self.tx.try_send(message(member_message::Message::Log(LogEntry {
                            heartbeats,
                            ..Default::default()
                        })));
                        heartbeats = 0;
                    }
                    if topology_dirty {
                        topology_dirty = false;
                        let m = self.topology();
                        if !self.send(m).await { return; }
                    }
                    if sources_dirty || last_sources.elapsed() >= SOURCES_EVERY {
                        sources_dirty = false;
                        last_sources = tokio::time::Instant::now();
                        let m = self.sources().await;
                        if !self.send(m).await { return; }
                    }
                }
            }
        }
    }

    /// One admin request of the master, answered when it is done: a slow one (a registry that does not answer) holds up nothing else.
    fn spawn_request(&self, r: hermes_proto::v1::Request) {
        let (router, state, tx) = (self.router.clone(), self.state.clone(), self.tx.clone());
        tokio::spawn(async move {
            let (status, body) = if link_allowed(&r.method, &r.path) {
                call(&router, &state, &r.method, &r.path, r.body).await
            } else {
                (
                    403,
                    br#"{"error":"the master may not do that on this hub"}"#.to_vec(),
                )
            };
            let _ = tx
                .send(message(member_message::Message::Response(LinkResponse {
                    id: r.id,
                    status: u32::from(status),
                    body,
                })))
                .await;
        });
    }
}

/// Makes a request on the hub's own router, carrying the proof that it comes from this process and not from the network.
async fn call(
    router: &Router,
    state: &Shared,
    method: &str,
    path: &str,
    body: Vec<u8>,
) -> (u16, Vec<u8>) {
    let request = HttpRequest::builder()
        .method(method)
        .uri(path)
        .header(LINK_HEADER, state.federation.link_secret())
        .header("x-requested-with", "hermes")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body));
    let Ok(request) = request else {
        return (400, br#"{"error":"invalid request"}"#.to_vec());
    };
    let Ok(response) = router.clone().oneshot(request).await;
    let status = response.status().as_u16();
    match axum::body::to_bytes(response.into_body(), BODY_LIMIT).await {
        Ok(bytes) => (status, bytes.to_vec()),
        Err(_) => (502, br#"{"error":"the answer is too big"}"#.to_vec()),
    }
}
