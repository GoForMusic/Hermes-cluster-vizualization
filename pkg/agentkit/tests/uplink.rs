//! The uplink against a real gRPC server: hello, snapshot, resync on request, and reconnecting after the connection drops.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hermes_agentkit::{Config, ISink, Timing, Uplink};
use hermes_proto::v1::agent_service_server::{AgentService, AgentServiceServer};
use hermes_proto::v1::{
    AgentMessage, Event, HubMessage, Node, NodeKind, Own, Resync, agent_message, event, hub_message,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tokio_stream::{Stream, StreamExt};
use tonic::transport::Server;
use tonic::{Request, Response, Status, Streaming};

type Out = Pin<Box<dyn Stream<Item = Result<HubMessage, Status>> + Send>>;
type ToAgent = Arc<Mutex<Option<mpsc::Sender<Result<HubMessage, Status>>>>>;

/// A hub that hands every message it receives to the test, and can be told to ask for a resync or to hang up.
#[derive(Clone)]
struct FakeHub {
    seen: mpsc::UnboundedSender<AgentMessage>,
    to_agent: ToAgent,
    token: &'static str,
}

#[tonic::async_trait]
impl AgentService for FakeHub {
    type SessionStream = Out;

    async fn session(
        &self,
        req: Request<Streaming<AgentMessage>>,
    ) -> Result<Response<Out>, Status> {
        if req
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            != Some(&format!("Bearer {}", self.token))
        {
            return Err(Status::unauthenticated("bad token"));
        }
        let (tx, rx) = mpsc::channel(4);
        *self.to_agent.lock().unwrap() = Some(tx);
        let mut inbound = req.into_inner();
        let seen = self.seen.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = inbound.next().await {
                let _ = seen.send(msg);
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

struct Rig {
    hub: FakeHub,
    seen: mpsc::UnboundedReceiver<AgentMessage>,
    addr: SocketAddr,
}

async fn start_hub(token: &'static str) -> Rig {
    let (seen_tx, seen) = mpsc::unbounded_channel();
    let hub = FakeHub {
        seen: seen_tx,
        to_agent: Arc::default(),
        token,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(
        Server::builder()
            .add_service(AgentServiceServer::new(hub.clone()))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );
    Rig { hub, seen, addr }
}

fn uplink(addr: SocketAddr, token: &str) -> Uplink {
    let env = [
        ("HUB_URL", format!("http://{addr}")),
        ("SOURCE_ID", "s1".into()),
        ("SOURCE_NAME", "lab".into()),
        ("TOKEN", token.into()),
        ("AGENT_ID", "pod-1".into()),
        ("AGENT_HOST", "k8s:node/w1".into()),
    ];
    let cfg = Config::from_lookup("node", "9.9.9", |k| {
        env.iter()
            .find(|(name, _)| *name == k)
            .map(|(_, v)| v.clone())
    })
    .unwrap();
    let fast = Timing {
        flush: Duration::from_millis(30),
        backoff_min: Duration::from_millis(30),
        backoff_max: Duration::from_millis(100),
    };
    Uplink::new(&cfg).unwrap().with_timing(fast)
}

fn node(id: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Workload.into(),
        name: id.into(),
        own: Own::Ok.into(),
        ..Default::default()
    }
}

/// The next message from the agent, or a panic after a few seconds.
async fn next(rig: &mut Rig) -> AgentMessage {
    tokio::time::timeout(Duration::from_secs(5), rig.seen.recv())
        .await
        .expect("the agent said nothing for 5 s")
        .expect("hub closed")
}

/// Skips heartbeats until a batch with events arrives.
async fn next_events(rig: &mut Rig) -> Vec<Event> {
    loop {
        if let Some(agent_message::Message::Batch(b)) = next(rig).await.message
            && !b.events.is_empty()
        {
            return b.events;
        }
    }
}

fn snapshot_ids(events: &[Event]) -> Option<Vec<String>> {
    events.iter().find_map(|e| match &e.kind {
        Some(event::Kind::Snapshot(s)) => Some(
            s.nodes
                .iter()
                .map(|n| format!("{}={:?}", n.id, n.own()))
                .collect(),
        ),
        _ => None,
    })
}

#[tokio::test]
async fn says_hello_then_sends_the_snapshot() {
    let mut rig = start_hub("t0k3n").await;
    let link = uplink(rig.addr, "t0k3n");
    link.set_topology(vec![node("a")], vec![]);
    tokio::spawn(link.run());

    let Some(agent_message::Message::Hello(hello)) = next(&mut rig).await.message else {
        panic!("the first message must be the hello")
    };
    assert_eq!(
        (
            hello.agent.as_str(),
            hello.host.as_str(),
            hello.version.as_str(),
            hello.collector.as_str(),
            hello.protocol
        ),
        ("pod-1", "k8s:node/w1", "9.9.9", "node", 1)
    );
    assert_eq!(
        snapshot_ids(&next_events(&mut rig).await).unwrap(),
        ["a=Ok"]
    );
}

#[tokio::test]
async fn a_resync_from_the_hub_sends_the_current_snapshot_again() {
    let mut rig = start_hub("t").await;
    let link = uplink(rig.addr, "t");
    link.set_topology(vec![node("a")], vec![]);
    tokio::spawn(link.clone().run());
    next_events(&mut rig).await; // the first snapshot

    link.status("a", Own::Crit, "OOMKilled");
    next_events(&mut rig).await; // the status change

    let tx = rig.hub.to_agent.lock().unwrap().clone().unwrap();
    tx.send(Ok(HubMessage {
        message: Some(hub_message::Message::Resync(Resync {})),
    }))
    .await
    .unwrap();
    assert_eq!(
        snapshot_ids(&next_events(&mut rig).await).unwrap(),
        ["a=Crit"],
        "the snapshot carries the change that came after it"
    );
}

#[tokio::test]
async fn reconnects_and_sends_everything_again_when_the_hub_hangs_up() {
    let mut rig = start_hub("t").await;
    let link = uplink(rig.addr, "t");
    link.set_topology(vec![node("a")], vec![]);
    link.alive(vec!["a".into()]);
    tokio::spawn(link.run());
    next_events(&mut rig).await;

    *rig.hub.to_agent.lock().unwrap() = None; // drops the sender: the stream ends

    loop {
        if let Some(agent_message::Message::Hello(_)) = next(&mut rig).await.message {
            break; // a new session
        }
    }
    let events = next_events(&mut rig).await;
    assert_eq!(snapshot_ids(&events).unwrap(), ["a=Ok"]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e.kind, Some(event::Kind::Alive(_))))
    );
}

#[tokio::test]
async fn a_wrong_token_never_gets_data_through() {
    let mut rig = start_hub("right").await;
    let link = uplink(rig.addr, "wrong");
    link.set_topology(vec![node("a")], vec![]);
    tokio::spawn(link.run());
    assert!(
        tokio::time::timeout(Duration::from_millis(600), rig.seen.recv())
            .await
            .is_err(),
        "the hub must not receive anything"
    );
}
