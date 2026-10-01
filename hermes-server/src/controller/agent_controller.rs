//! The agents' side of the hub: one long-lived gRPC stream per agent.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hermes_proto::v1::agent_service_server::{AgentService, AgentServiceServer};
use hermes_proto::v1::{AgentMessage, Hello, HubMessage, Resync, agent_message, hub_message};
use prost::Message;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::info;

use crate::model::Source;
use crate::services::{
    Event, HelloInfo, IIngestService, NewEntry, SharedCommLog, SharedUpgrades, describe_batch,
};

/// A first snapshot of a big cluster is far above tonic's default limit of 4 MiB.
const MAX_MESSAGE: usize = 32 * 1024 * 1024;
/// The first message must come this soon after the stream opens.
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// A source that was deleted while its agent is connected loses the stream within this time.
const REAUTH_EVERY: Duration = Duration::from_secs(30);

pub(super) fn routes(
    ingest: Arc<dyn IIngestService>,
    upgrades: SharedUpgrades,
    log: SharedCommLog,
) -> Router {
    let service = AgentServiceServer::new(Agents {
        ingest,
        upgrades,
        log,
    })
    .max_decoding_message_size(MAX_MESSAGE);
    tonic::service::Routes::new(service).into_axum_router()
}

struct Agents {
    ingest: Arc<dyn IIngestService>,
    upgrades: SharedUpgrades,
    log: SharedCommLog,
}

type Out = Pin<Box<dyn Stream<Item = Result<HubMessage, Status>> + Send>>;

fn bearer(req: &Request<Streaming<AgentMessage>>) -> &str {
    req.metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
}

#[tonic::async_trait]
impl AgentService for Agents {
    type SessionStream = Out;

    async fn session(
        &self,
        req: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Out>, Status> {
        let token = bearer(&req).to_string();
        let source = self
            .ingest
            .authenticate(&token)
            .ok_or_else(|| Status::unauthenticated("unknown agent token"))?;
        let mut inbound = req.into_inner();
        let (tx, rx) = mpsc::channel(4);
        let ingest = self.ingest.clone();
        let upgrades = self.upgrades.clone();
        let log = self.log.clone();

        tokio::spawn(async move {
            let Some(hello) = recv_hello(&mut inbound).await else {
                let _ = tx
                    .send(Err(Status::invalid_argument(
                        "the first message must be a hello",
                    )))
                    .await;
                return;
            };
            info!(
                "agent {} connected to source {} (version {}, collector {}, protocol {})",
                hello.agent, source.id, hello.version, hello.collector, hello.protocol
            );
            ingest.hello(
                &source,
                &hello.agent,
                &hello.host,
                HelloInfo {
                    version: &hello.version,
                    collector: &hello.collector,
                    protocol: hello.protocol,
                },
            );
            let session = upgrades.attach(&source.id, &hello.agent, &hello.features, tx.clone());
            let trail = Trail {
                log: &*log,
                source: &source,
                agent: &hello.agent,
            };
            trail.record("in", "connected", "hello".into(), u64::try_from(hello.encoded_len()).unwrap_or(0), || {
                json!({"agent": hello.agent, "host": hello.host, "version": hello.version, "collector": hello.collector, "protocol": hello.protocol, "features": hello.features})
            });
            let why = run_session(&*ingest, &*upgrades, &trail, &hello, &token, &tx, inbound).await;
            trail.record(
                "link",
                "disconnected",
                why.into(),
                0,
                || json!({"reason": why}),
            );
            upgrades.detach(&source.id, &hello.agent, session);
            info!(
                "agent {} disconnected from source {}",
                hello.agent, source.id
            );
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

/// Waits for the mandatory first message of a session. Anything else — nothing within `HELLO_WITHIN`, or a message that isn't a
/// hello — is a protocol violation, left for the caller to report.
async fn recv_hello(inbound: &mut Streaming<AgentMessage>) -> Option<Hello> {
    match tokio::time::timeout(HELLO_WITHIN, inbound.message()).await {
        Ok(Ok(Some(AgentMessage {
            message: Some(agent_message::Message::Hello(h)),
        }))) => Some(h),
        _ => None,
    }
}

/// Writes what passes between the hub and one agent into the communication log, when it is on. The content is built only then.
struct Trail<'a> {
    log: &'a dyn crate::services::ICommLog,
    source: &'a Source,
    agent: &'a str,
}

impl Trail<'_> {
    fn record(
        &self,
        dir: &'static str,
        kind: &'static str,
        summary: String,
        bytes: u64,
        body: impl FnOnce() -> serde_json::Value,
    ) {
        if !self.log.enabled() {
            return;
        }
        self.log.record(NewEntry {
            ts: crate::database::now_ms(),
            source: self.source.id.clone(),
            source_name: self.source.name.clone(),
            agent: self.agent.to_string(),
            dir,
            kind,
            summary,
            bytes,
            body: body(),
        });
    }
}

impl Trail<'_> {
    fn heartbeat(&self) {
        self.log.heartbeat();
    }

    /// A batch that says something: what kinds of events, and each of them whole.
    fn batch(&self, events: &[Event], bytes: u64) {
        if !self.log.enabled() {
            return;
        }
        let (summary, body) = describe_batch(events);
        self.record("in", "batch", summary, bytes, || body);
    }
}

/// Applies every batch the agent sends after its hello, until it disconnects or the source it authenticated with is gone
/// (checked every `REAUTH_EVERY`, since a deleted source's token must stop working without waiting for the agent to reconnect).
async fn run_session(
    ingest: &dyn IIngestService,
    upgrades: &dyn crate::services::IUpgradeService,
    trail: &Trail<'_>,
    hello: &Hello,
    token: &str,
    tx: &mpsc::Sender<Result<HubMessage, Status>>,
    mut inbound: Streaming<AgentMessage>,
) -> &'static str {
    // The hub asks for a snapshot once, and again only after the agent has answered: not on every batch while it is on its way.
    let mut asked = false;
    let mut reauth = tokio::time::interval(REAUTH_EVERY);
    reauth.tick().await;
    loop {
        tokio::select! {
            msg = inbound.next() => {
                let Some(Ok(msg)) = msg else { return "the agent closed the connection" };
                let batch = match msg.message {
                    Some(agent_message::Message::Batch(batch)) => batch,
                    Some(agent_message::Message::UpgradeStatus(status)) => {
                        trail.record("in", "upgrade_status", format!("{:?} {}", status.state(), status.version), u64::try_from(status.encoded_len()).unwrap_or(0), || {
                            json!({"state": format!("{:?}", status.state()), "version": status.version, "message": status.message})
                        });
                        upgrades.report(&trail.source.id, &hello.agent, &status);
                        continue;
                    }
                    _ => continue,
                };
                let bytes = u64::try_from(batch.encoded_len()).unwrap_or(0);
                let events: Vec<Event> = batch.events.into_iter().filter_map(Event::from_proto).collect();
                if events.is_empty() {
                    trail.heartbeat();
                } else {
                    trail.batch(&events, bytes);
                }
                if events.iter().any(|e| matches!(e, Event::Snapshot { .. })) {
                    asked = false;
                }
                let resync = ingest.handle(trail.source, &hello.agent, &hello.host, events);
                if resync && !asked {
                    asked = true;
                    let resync = HubMessage { message: Some(hub_message::Message::Resync(Resync {})) };
                    trail.record("out", "resync", "send the snapshot again".into(), 0, || json!({"why": "the hub has no topology for this agent"}));
                    if tx.send(Ok(resync)).await.is_err() {
                        return "the hub could not write to the agent";
                    }
                }
            }
            _ = reauth.tick() => {
                if ingest.authenticate(token).is_none_or(|s| s.id != trail.source.id) {
                    let _ = tx.send(Err(Status::unauthenticated("the source was removed"))).await;
                    return "the source was removed";
                }
            }
        }
    }
}
