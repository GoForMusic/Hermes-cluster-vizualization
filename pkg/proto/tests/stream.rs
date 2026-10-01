//! The contract works end to end: a real gRPC stream over TCP, agent token in the metadata, snapshot up, resync down.

use std::pin::Pin;

use hermes_proto::PROTOCOL;
use hermes_proto::v1::agent_service_client::AgentServiceClient;
use hermes_proto::v1::agent_service_server::{AgentService, AgentServiceServer};
use hermes_proto::v1::{
    AgentMessage, Batch, Event, Hello, HubMessage, Node, NodeKind, Own, Provider, Resync, Snapshot,
};
use hermes_proto::v1::{agent_message, event, hub_message};
use prost::Message;
use prost_types::{Struct, Value, value::Kind};
use tokio::net::TcpListener;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tokio_stream::{Stream, StreamExt};
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status, Streaming};

const TOKEN: &str = "s3cret";

/// A hub that only checks the token, reads the hello and asks for a resync after the first snapshot.
struct FakeHub;

type Out = Pin<Box<dyn Stream<Item = Result<HubMessage, Status>> + Send>>;

#[tonic::async_trait]
impl AgentService for FakeHub {
    type SessionStream = Out;

    async fn session(
        &self,
        req: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Out>, Status> {
        let auth = req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if auth != format!("Bearer {TOKEN}") {
            return Err(Status::unauthenticated("bad token"));
        }
        let mut inbound = req.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            let Some(Ok(AgentMessage {
                message: Some(agent_message::Message::Hello(hello)),
            })) = inbound.next().await
            else {
                return;
            };
            assert_eq!(hello.protocol, PROTOCOL);
            assert_eq!(hello.agent, "pod-1");
            while let Some(Ok(msg)) = inbound.next().await {
                if let Some(agent_message::Message::Batch(b)) = msg.message {
                    let has_snapshot = b
                        .events
                        .iter()
                        .any(|e| matches!(e.kind, Some(event::Kind::Snapshot(_))));
                    if has_snapshot {
                        let _ = tx
                            .send(Ok(HubMessage {
                                message: Some(hub_message::Message::Resync(Resync {})),
                            }))
                            .await;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

async fn start_hub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(
        Server::builder()
            .add_service(AgentServiceServer::new(FakeHub))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    format!("http://{addr}")
}

fn node() -> Node {
    let meta = Struct {
        fields: [(
            "image".to_string(),
            Value {
                kind: Some(Kind::StringValue("nginx:1.27".into())),
            },
        )]
        .into(),
    };
    Node {
        id: "k8s:default/web".into(),
        kind: NodeKind::Workload.into(),
        name: "web".into(),
        parent: Some("k8s:node/w1".into()),
        provider: Provider::Kubernetes.into(),
        own: Own::Ok.into(),
        since: 1_700_000_000_000,
        m: [("cpu".to_string(), 12.5)].into(),
        meta: Some(meta),
        ..Default::default()
    }
}

fn msg(m: agent_message::Message) -> AgentMessage {
    AgentMessage { message: Some(m) }
}

#[tokio::test]
async fn snapshot_goes_up_and_resync_comes_down() {
    let url = start_hub().await;
    let mut client =
        AgentServiceClient::new(Channel::from_shared(url).unwrap().connect().await.unwrap());

    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let mut req = Request::new(ReceiverStream::new(rx));
    req.metadata_mut()
        .insert("authorization", format!("Bearer {TOKEN}").parse().unwrap());
    let mut down = client.session(req).await.unwrap().into_inner();

    tx.send(msg(agent_message::Message::Hello(Hello {
        protocol: PROTOCOL,
        agent: "pod-1".into(),
        ..Default::default()
    })))
    .await
    .unwrap();
    tx.send(msg(agent_message::Message::Batch(Batch { events: vec![] })))
        .await
        .unwrap(); // a heartbeat
    let snapshot = Event {
        kind: Some(event::Kind::Snapshot(Snapshot {
            nodes: vec![node()],
            edges: vec![],
        })),
    };
    tx.send(msg(agent_message::Message::Batch(Batch {
        events: vec![snapshot],
    })))
    .await
    .unwrap();

    let got = tokio::time::timeout(std::time::Duration::from_secs(5), down.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(got.message, Some(hub_message::Message::Resync(_))));
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let url = start_hub().await;
    let mut client =
        AgentServiceClient::new(Channel::from_shared(url).unwrap().connect().await.unwrap());
    let (_tx, rx) = tokio::sync::mpsc::channel::<AgentMessage>(1);
    let mut req = Request::new(ReceiverStream::new(rx));
    req.metadata_mut()
        .insert("authorization", "Bearer nope".parse().unwrap());
    let err = client.session(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

/// A newer agent may send fields this hub has never heard of: they are skipped, not an error.
#[test]
fn unknown_fields_are_ignored() {
    let mut bytes = node().encode_to_vec();
    prost::encoding::encode_key(99, prost::encoding::WireType::LengthDelimited, &mut bytes);
    prost::encoding::encode_varint(3, &mut bytes);
    bytes.extend_from_slice(b"new");
    let decoded = Node::decode(bytes.as_slice()).expect("unknown field must be skipped");
    assert_eq!(decoded.name, "web");
    assert_eq!(decoded.parent.as_deref(), Some("k8s:node/w1"));
}
