use super::*;
use hermes_proto::v1::hub_message::Message;

/// An agent as the hub sees it: the other end of its channel.
fn agent(
    svc: &LogServiceImp,
    source: &str,
    name: &str,
    features: &[&str],
) -> (u64, mpsc::Receiver<Result<HubMessage, tonic::Status>>) {
    let (tx, rx) = mpsc::channel(16);
    let features: Vec<String> = features.iter().map(ToString::to_string).collect();
    (svc.attach(source, name, &features, tx), rx)
}

fn opts(name: &str) -> LogOptions {
    LogOptions {
        namespace: "shop".into(),
        name: name.into(),
        tail: 50,
        ..LogOptions::default()
    }
}

async fn asked(rx: &mut mpsc::Receiver<Result<HubMessage, tonic::Status>>) -> HubMessage {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

fn lines(id: u64, l: &[&str]) -> LogChunk {
    LogChunk {
        id,
        lines: l.iter().map(ToString::to_string).collect(),
        ..Default::default()
    }
}

#[tokio::test]
async fn only_an_agent_installed_to_read_logs_is_asked() {
    let svc = LogServiceImp::new();
    let (_, mut plain) = agent(&svc, "s1", "a", &["upgrade"]);
    assert!(!svc.can_read("s1"));
    assert_eq!(
        svc.open("s1", &opts("web")).await.err(),
        Some(LogOpenError::NoCapableAgent)
    );
    assert!(
        plain.try_recv().is_err(),
        "the agent that cannot read was not asked"
    );
    let (_, _reader) = agent(&svc, "s1", "b", &["logs"]);
    assert!(svc.can_read("s1"));
    assert!(!svc.can_read("s2"));
}

#[tokio::test]
async fn the_lines_that_come_back_reach_the_viewer_in_order_and_the_end_closes_it() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!("not a request")
    };
    assert_eq!(
        (req.namespace.as_str(), req.name.as_str(), req.tail),
        ("shop", "web", 50)
    );
    svc.chunk("s1", "a", lines(req.id, &["one", "two"]));
    let mut stream = opening.await.unwrap().unwrap();
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Lines(vec!["one".into(), "two".into()]))
    );
    svc.chunk("s1", "a", lines(req.id, &["three"]));
    svc.chunk(
        "s1",
        "a",
        LogChunk {
            id: req.id,
            end: true,
            ..Default::default()
        },
    );
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Lines(vec!["three".into()]))
    );
    assert_eq!(stream.next().await, Some(LogEvent::End));
    assert_eq!(stream.next().await, None);
    drop(stream);
    assert!(a.try_recv().is_err(), "a log that ended is not cancelled");
}

#[tokio::test]
async fn an_agent_that_is_not_the_one_for_the_workload_is_skipped_for_the_next() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let (_, mut b) = agent(&svc, "s1", "b", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(first)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk(
        "s1",
        "a",
        LogChunk {
            id: first.id,
            error: "not on this node".into(),
            not_here: true,
            ..Default::default()
        },
    );
    let Some(Message::LogRequest(second)) = asked(&mut b).await.message else {
        panic!()
    };
    assert_ne!(first.id, second.id);
    svc.chunk("s1", "b", lines(second.id, &["found it"]));
    let mut stream = opening.await.unwrap().unwrap();
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Lines(vec!["found it".into()]))
    );
}

#[tokio::test]
async fn when_nobody_has_it_the_reasons_are_given() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk(
        "s1",
        "a",
        LogChunk {
            id: req.id,
            error: "no such pod".into(),
            not_here: true,
            ..Default::default()
        },
    );
    assert_eq!(
        opening.await.unwrap().err(),
        Some(LogOpenError::NotFound("no such pod".into()))
    );
}

#[tokio::test]
async fn a_failure_ends_the_log_with_its_reason() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk("s1", "a", lines(req.id, &["x"]));
    let mut stream = opening.await.unwrap().unwrap();
    svc.chunk(
        "s1",
        "a",
        LogChunk {
            id: req.id,
            error: "the pod is gone".into(),
            ..Default::default()
        },
    );
    assert_eq!(stream.next().await, Some(LogEvent::Lines(vec!["x".into()])));
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Failed("the pod is gone".into()))
    );
    assert_eq!(stream.next().await, None);
}

#[tokio::test]
async fn a_viewer_that_leaves_cancels_the_request_at_the_agent() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk("s1", "a", lines(req.id, &["x"]));
    let stream = opening.await.unwrap().unwrap();
    drop(stream);
    let Some(Message::LogCancel(c)) = asked(&mut a).await.message else {
        panic!("not a cancel")
    };
    assert_eq!(c.id, req.id);
    // what the agent still sends is ignored
    svc.chunk("s1", "a", lines(req.id, &["late"]));
}

#[tokio::test]
async fn nobody_answers_for_another_agents_request() {
    let svc = LogServiceImp::new();
    let (_, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let (_, _b) = agent(&svc, "s1", "b", &["upgrade"]);
    let (_, _c) = agent(&svc, "s2", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk("s1", "b", lines(req.id, &["from the wrong agent"]));
    svc.chunk("s2", "a", lines(req.id, &["from another source"]));
    svc.chunk("s1", "a", lines(req.id, &["from the right one"]));
    let mut stream = opening.await.unwrap().unwrap();
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Lines(vec!["from the right one".into()]))
    );
}

#[tokio::test]
async fn an_agent_that_disconnects_ends_what_was_read_through_it() {
    let svc = LogServiceImp::new();
    let (session, mut a) = agent(&svc, "s1", "a", &["logs"]);
    let opening = tokio::spawn({
        let svc = svc.clone();
        async move { svc.open("s1", &opts("web")).await }
    });
    let Some(Message::LogRequest(req)) = asked(&mut a).await.message else {
        panic!()
    };
    svc.chunk("s1", "a", lines(req.id, &["x"]));
    let mut stream = opening.await.unwrap().unwrap();
    svc.detach("s1", "a", session);
    assert_eq!(stream.next().await, Some(LogEvent::Lines(vec!["x".into()])));
    assert_eq!(
        stream.next().await,
        Some(LogEvent::Failed("the agent disconnected".into()))
    );
    assert!(!svc.can_read("s1"));
}

#[tokio::test]
async fn a_newer_session_of_an_agent_is_not_ended_by_the_older_one_closing() {
    let svc = LogServiceImp::new();
    let (old, _a) = agent(&svc, "s1", "a", &["logs"]);
    let (_new, _a2) = agent(&svc, "s1", "a", &["logs"]);
    svc.detach("s1", "a", old);
    assert!(svc.can_read("s1"));
}
