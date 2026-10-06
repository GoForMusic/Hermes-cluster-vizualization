use tokio::sync::mpsc;

use super::*;

fn agent(version: &str, seen: u64) -> AgentInfo {
    AgentInfo {
        id: "a".into(),
        version: version.into(),
        collector: "kubernetes".into(),
        host: String::new(),
        seen_ago: Duration::from_secs(seen),
        protocol: hermes_proto::PROTOCOL,
    }
}

fn attach(
    svc: &UpgradeServiceImp,
    source: &str,
    agent: &str,
    features: &[&str],
) -> (u64, mpsc::Receiver<Result<HubMessage, Status>>) {
    let (tx, rx) = mpsc::channel(4);
    let features: Vec<String> = features.iter().map(ToString::to_string).collect();
    (svc.attach(source, agent, &features, tx), rx)
}

#[test]
fn only_an_agent_that_said_it_can_is_told_and_it_is_told_the_version_alone() {
    let svc = UpgradeServiceImp::new();
    let (_, mut plain) = attach(&svc, "s1", "old", &[]);
    assert!(!svc.can_upgrade("s1"));
    assert_eq!(
        svc.request("s1", "1.0.5"),
        Err(UpgradeError::NoCapableAgent)
    );
    let (_, mut capable) = attach(&svc, "s1", "new", &["upgrade"]);
    assert!(svc.can_upgrade("s1") && !svc.can_upgrade("s2"));
    assert_eq!(svc.request("s1", "1.0.5"), Ok(1));
    let Some(hub_message::Message::Upgrade(u)) = capable.try_recv().unwrap().unwrap().message
    else {
        panic!("no upgrade")
    };
    assert_eq!(u.version, "1.0.5");
    assert!(
        plain.try_recv().is_err(),
        "the read-only agent hears nothing"
    );
}

#[test]
fn a_version_that_is_not_one_is_refused_and_nothing_is_sent() {
    let svc = UpgradeServiceImp::new();
    let (_, mut rx) = attach(&svc, "s1", "a", &["upgrade"]);
    for bad in ["", "latest", "1.0", "1.0.5; rm -rf /", "../x"] {
        assert_eq!(
            svc.request("s1", bad),
            Err(UpgradeError::BadVersion),
            "{bad:?}"
        );
    }
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_reconnect_is_not_undone_by_the_old_session_ending_late() {
    let svc = UpgradeServiceImp::new();
    let (old, _rx1) = attach(&svc, "s1", "a", &["upgrade"]);
    let (_new, _rx2) = attach(&svc, "s1", "a", &["upgrade"]);
    svc.detach("s1", "a", old);
    assert!(svc.can_upgrade("s1"));
}

#[test]
fn done_pending_and_failed_are_told_by_what_the_agents_run() {
    let svc = UpgradeServiceImp::new();
    let (_, _rx) = attach(&svc, "s1", "a", &["upgrade"]);
    assert_eq!(
        svc.view("s1", &[agent("1.0.0", 1)]),
        None,
        "nothing asked yet"
    );
    svc.request("s1", "1.0.5").unwrap();
    let state = |agents: &[AgentInfo]| svc.view("s1", agents).unwrap().state;
    assert_eq!(state(&[agent("1.0.0", 1)]), "pending");
    assert_eq!(
        state(&[agent("1.0.5", 1), agent("1.0.0", 1)]),
        "pending",
        "one instance is still on the old version"
    );
    assert_eq!(
        state(&[agent("1.0.5", 1), agent("1.0.0", 600)]),
        "done",
        "a replaced instance that fell silent does not count"
    );
    assert_eq!(state(&[agent("v1.0.5", 1)]), "done");
    assert_eq!(state(&[]), "pending", "nobody is reporting yet");
    // "done" is news: it goes quiet after a while, a failure does not
    let quiet = Instant::now() + DONE_SHOWN_FOR + Duration::from_secs(1);
    assert_eq!(
        svc.view_at(Instant::now(), "s1", &[agent("1.0.5", 1)])
            .unwrap()
            .state,
        "done"
    );
    assert_eq!(svc.view_at(quiet, "s1", &[agent("1.0.5", 1)]), None);
    // and when it takes too long
    let late = Instant::now() + GIVE_UP_AFTER + Duration::from_secs(1);
    let view = svc.view_at(late, "s1", &[agent("1.0.0", 1)]).unwrap();
    assert_eq!(view.state, "failed");
    assert!(view.message.contains("still on 1.0.0"), "{}", view.message);
}

#[test]
fn a_failure_reported_by_the_agent_is_shown_with_its_reason_and_a_new_request_starts_over() {
    let svc = UpgradeServiceImp::new();
    let (_, _rx) = attach(&svc, "s1", "a", &["upgrade"]);
    svc.request("s1", "1.0.5").unwrap();
    svc.report(
        "s1",
        "a",
        &UpgradeStatus {
            state: UpgradeState::Started.into(),
            version: "1.0.5".into(),
            message: String::new(),
        },
    );
    assert_eq!(
        svc.view("s1", &[agent("1.0.0", 1)]).unwrap().state,
        "pending"
    );
    svc.report(
        "s1",
        "a",
        &UpgradeStatus {
            state: UpgradeState::Failed.into(),
            version: "1.0.5".into(),
            message: "no permission".into(),
        },
    );
    let v = svc.view("s1", &[agent("1.0.0", 1)]).unwrap();
    assert_eq!(
        (v.state.as_str(), v.message.as_str()),
        ("failed", "no permission")
    );
    svc.request("s1", "1.0.6").unwrap();
    assert_eq!(
        svc.view("s1", &[agent("1.0.0", 1)]).unwrap().state,
        "pending"
    );
    svc.report(
        "s1",
        "a",
        &UpgradeStatus {
            state: UpgradeState::Skipped.into(),
            version: "1.0.6".into(),
            message: "a worker".into(),
        },
    );
    assert_eq!(
        svc.view("s1", &[agent("1.0.0", 1)]).unwrap().state,
        "pending",
        "a worker skipping is not a failure"
    );
    svc.forget("s1");
    assert_eq!(svc.view("s1", &[]), None);
}

#[test]
fn removing_a_source_tells_its_agents_at_once_and_leaves_the_others_alone() {
    let svc = UpgradeServiceImp::new();
    let (_, mut gone) = attach(&svc, "s1", "a", &[]);
    let (_, mut other) = attach(&svc, "s2", "b", &[]);
    svc.forget("s1");
    let told = gone.try_recv().expect("the agent is told");
    assert_eq!(told.unwrap_err().code(), tonic::Code::Unauthenticated);
    assert!(
        other.try_recv().is_err(),
        "another source's agent hears nothing"
    );
}
