//! The uplink against a real gRPC server: hello, snapshot, resync on request, and reconnecting after the connection drops.

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hermes_agentkit::{Config, ILogs, ISink, LogError, LogQuery, Timing, Uplink};
use hermes_proto::v1::agent_service_server::{AgentService, AgentServiceServer};
use hermes_proto::v1::{
    AgentMessage, Event, HubMessage, LogCancel, LogRequest, Node, NodeKind, Own, Resync,
    agent_message, event, hub_message,
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

// ---- logs ------------------------------------------------------------------------------------------------------

/// What an agent that can read logs does here: a log of three lines, one that never ends, and a workload that is not on this agent.
struct FakeLogs {
    stopped: Arc<Mutex<Vec<String>>>,
}

#[tonic::async_trait]
impl ILogs for FakeLogs {
    async fn read(&self, q: LogQuery, out: mpsc::Sender<Vec<String>>) -> Result<(), LogError> {
        match q.name.as_str() {
            "elsewhere" => Err(LogError::NotHere("this node does not run it".into())),
            "broken" => Err(LogError::Failed("the pod is gone".into())),
            "endless" => {
                struct Note(Arc<Mutex<Vec<String>>>);
                impl Drop for Note {
                    fn drop(&mut self) {
                        self.0.lock().unwrap().push("stopped".into());
                    }
                }
                let _note = Note(self.stopped.clone());
                loop {
                    if out.send(vec!["tick".into()]).await.is_err() {
                        return Ok(());
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
            _ => {
                let _ = out
                    .send(vec![
                        format!(
                            "{}/{} tail={} previous={}",
                            q.namespace, q.name, q.tail, q.previous
                        ),
                        "second".into(),
                    ])
                    .await;
                let _ = out.send(vec!["third".into()]).await;
                Ok(())
            }
        }
    }
}

fn ask(id: u64, name: &str, tail: u32, previous: bool) -> HubMessage {
    HubMessage {
        message: Some(hub_message::Message::LogRequest(LogRequest {
            id,
            namespace: "shop".into(),
            name: name.into(),
            tail,
            previous,
            ..Default::default()
        })),
    }
}

/// The chunks of one request until it ends: (lines, error, not_here, ended).
async fn chunks_of(rig: &mut Rig, id: u64) -> (Vec<String>, String, bool, bool) {
    let (mut lines, mut error, mut not_here, mut end) = (vec![], String::new(), false, false);
    while !end && error.is_empty() {
        if let Some(agent_message::Message::LogChunk(c)) = next(rig).await.message {
            assert_eq!(c.id, id);
            lines.extend(c.lines);
            (error, not_here, end) = (c.error, c.not_here, c.end);
        }
    }
    (lines, error, not_here, end)
}

async fn hub_says(rig: &Rig, message: HubMessage) {
    let tx = rig.hub.to_agent.lock().unwrap().clone().unwrap();
    tx.send(Ok(message)).await.unwrap();
}

#[tokio::test]
async fn an_agent_that_reads_logs_says_so_streams_the_lines_and_ends() {
    let mut rig = start_hub("t").await;
    let link = uplink(rig.addr, "t").with_logs(Arc::new(FakeLogs {
        stopped: Arc::default(),
    }));
    tokio::spawn(link.run());
    let Some(agent_message::Message::Hello(hello)) = next(&mut rig).await.message else {
        panic!("no hello")
    };
    assert_eq!(hello.features, ["logs"]);

    hub_says(&rig, ask(5, "web-1", 100, true)).await;
    let (lines, error, _, ended) = chunks_of(&mut rig, 5).await;
    assert_eq!(
        lines,
        ["shop/web-1 tail=100 previous=true", "second", "third"]
    );
    assert_eq!((error.as_str(), ended), ("", true));

    // the number of lines asked for is the agent's to limit
    hub_says(&rig, ask(6, "web-1", 1_000_000, false)).await;
    let (lines, ..) = chunks_of(&mut rig, 6).await;
    assert_eq!(lines[0], "shop/web-1 tail=5000 previous=false");
}

#[tokio::test]
async fn a_log_that_cannot_be_read_says_why_and_whether_to_ask_somebody_else() {
    let mut rig = start_hub("t").await;
    let link = uplink(rig.addr, "t").with_logs(Arc::new(FakeLogs {
        stopped: Arc::default(),
    }));
    tokio::spawn(link.run());
    hub_says_after_hello(&mut rig).await;
    hub_says(&rig, ask(1, "elsewhere", 10, false)).await;
    let (_, error, not_here, ended) = chunks_of(&mut rig, 1).await;
    assert_eq!(
        (error.as_str(), not_here, ended),
        ("this node does not run it", true, false)
    );
    hub_says(&rig, ask(2, "broken", 10, false)).await;
    let (_, error, not_here, _) = chunks_of(&mut rig, 2).await;
    assert_eq!((error.as_str(), not_here), ("the pod is gone", false));
}

async fn hub_says_after_hello(rig: &mut Rig) {
    loop {
        if let Some(agent_message::Message::Hello(_)) = next(rig).await.message {
            return;
        }
    }
}

#[tokio::test]
async fn an_agent_that_was_not_installed_to_read_logs_never_does_and_says_so() {
    let mut rig = start_hub("t").await;
    tokio::spawn(uplink(rig.addr, "t").run());
    let Some(agent_message::Message::Hello(hello)) = next(&mut rig).await.message else {
        panic!("no hello")
    };
    assert!(hello.features.is_empty());
    hub_says(&rig, ask(9, "web-1", 10, false)).await;
    let (_, error, not_here, _) = chunks_of(&mut rig, 9).await;
    assert!(error.contains("not installed"), "{error}");
    assert!(not_here, "somebody else may be");
}

#[tokio::test]
async fn a_log_that_is_followed_stops_when_the_hub_cancels_it_and_when_the_session_ends() {
    let mut rig = start_hub("t").await;
    let stopped = Arc::new(Mutex::new(Vec::new()));
    let link = uplink(rig.addr, "t").with_logs(Arc::new(FakeLogs {
        stopped: stopped.clone(),
    }));
    tokio::spawn(link.run());
    hub_says_after_hello(&mut rig).await;

    hub_says(&rig, ask(3, "endless", 10, false)).await;
    loop {
        if let Some(agent_message::Message::LogChunk(c)) = next(&mut rig).await.message
            && c.lines == ["tick"]
        {
            break;
        }
    }
    hub_says(
        &rig,
        HubMessage {
            message: Some(hub_message::Message::LogCancel(LogCancel { id: 3 })),
        },
    )
    .await;
    for _ in 0..50 {
        if !stopped.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        *stopped.lock().unwrap(),
        ["stopped"],
        "the reader was stopped, not left running"
    );

    // one that is open when the hub hangs up is stopped too
    hub_says(&rig, ask(4, "endless", 10, false)).await;
    loop {
        if let Some(agent_message::Message::LogChunk(c)) = next(&mut rig).await.message
            && c.id == 4
        {
            break;
        }
    }
    *rig.hub.to_agent.lock().unwrap() = None;
    for _ in 0..100 {
        if stopped.lock().unwrap().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(stopped.lock().unwrap().len(), 2);
}
