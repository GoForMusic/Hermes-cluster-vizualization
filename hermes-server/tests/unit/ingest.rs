use tokio::sync::broadcast::error::TryRecvError;

use super::*;
use crate::database::Repositories;
use crate::services::{GuardImp, StoreImp};

struct Rig {
    svc: IngestServiceImp,
    store: Arc<dyn IStore>,
    db: Arc<dyn ISourceDAO>,
    t0: Instant,
}

fn rig() -> Rig {
    let store: Arc<dyn IStore> = Arc::new(StoreImp::new());
    let db = Repositories::sqlite_in_memory().unwrap().sources;
    let guard: Arc<dyn IGuard> = Arc::new(GuardImp::new(store.clone(), db.clone()));
    let t0 = Instant::now();
    Rig {
        svc: IngestServiceImp::started_at(guard, store.clone(), db.clone(), t0),
        store,
        db,
        t0,
    }
}

impl Rig {
    fn source(&self, id: &str, state: &str) -> Source {
        let s = Source {
            id: id.into(),
            name: format!("name-{id}"),
            kind: "Docker Swarm (agent)".into(),
            state: state.into(),
            secret: format!("tok-{id}"),
            ..Default::default()
        };
        self.db.insert_source(&s).unwrap();
        s
    }

    /// A source of Docker machines, which is not in a swarm.
    fn docker_source(&self, id: &str, state: &str) -> Source {
        let s = Source {
            id: id.into(),
            name: format!("name-{id}"),
            kind: "Docker (agent)".into(),
            state: state.into(),
            secret: format!("tok-{id}"),
            ..Default::default()
        };
        self.db.insert_source(&s).unwrap();
        s
    }

    fn state(&self, id: &str) -> (String, String) {
        let s = self
            .db
            .list_sources()
            .unwrap()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap();
        (s.state, s.info)
    }

    fn at(&self, secs: u64) -> Instant {
        self.t0 + Duration::from_secs(secs)
    }
}

fn node(id: &str, kind: &str, parent: Option<&str>) -> Node {
    Node {
        id: id.into(),
        kind: kind.into(),
        name: id.into(),
        parent: parent.map(Into::into),
        own: "ok".into(),
        status: "ok".into(),
        ..Default::default()
    }
}

fn snapshot(ids: &[&str]) -> Event {
    Event::Snapshot {
        nodes: ids.iter().map(|i| node(i, "host", None)).collect(),
        edges: vec![],
    }
}

#[test]
fn a_snapshot_makes_the_source_connected() {
    let r = rig();
    let src = r.source("s", "pending");
    assert!(
        !r.svc
            .handle_at(r.at(0), &src, "a1", "", vec![snapshot(&["h1", "h2"])])
    );
    assert_eq!(
        r.state("s"),
        ("connected".into(), "agent reporting · 2 nodes".into())
    );
    assert_eq!(r.store.nodes().len(), 2);
}

#[test]
fn an_agent_is_told_to_resend_only_while_the_hub_has_nothing_of_the_source() {
    let r = rig();
    let src = r.source("s", "pending");
    assert!(
        r.svc.handle_at(r.at(0), &src, "a1", "", vec![]),
        "the hub restarted: a heartbeat is answered with a request"
    );
    r.svc
        .handle_at(r.at(1), &src, "a1", "", vec![snapshot(&["h1"])]);
    assert!(!r.svc.handle_at(r.at(2), &src, "a1", "", vec![]));
}

#[test]
fn the_token_finds_its_agent_source_and_nothing_else() {
    let r = rig();
    r.source("s", "pending");
    r.db.insert_source(&Source {
        id: "k".into(),
        name: "k".into(),
        kind: "Kubernetes".into(),
        secret: "kubeconfig".into(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(r.svc.authenticate("tok-s").unwrap().id, "s");
    assert!(r.svc.authenticate("tok-x").is_none());
    assert!(r.svc.authenticate("").is_none());
    assert!(
        r.svc.authenticate("kubeconfig").is_none(),
        "a pull source's credential is not an agent token"
    );
}

#[test]
fn a_silent_agent_makes_the_source_down_and_its_nodes_stale_until_it_is_back() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "a1", "", vec![snapshot(&["h1"])]);
    r.svc.check(r.at(15));
    assert_eq!(r.state("s").0, "connected");
    assert!(!r.store.nodes()[0].stale);

    r.svc.check(r.at(25));
    assert_eq!(
        r.state("s"),
        (
            "error".into(),
            "the agent that reports the cluster silent for 25s".into()
        )
    );
    assert!(
        r.store.nodes()[0].stale,
        "the last state is shown as out of date, not as live"
    );

    // it comes back with a heartbeat only: the hub asks for a fresh snapshot and stops showing the source as down
    assert!(r.svc.handle_at(r.at(40), &src, "a1", "", vec![]));
    assert_eq!(
        r.state("s"),
        ("connected".into(), "agent reporting again".into())
    );
    assert!(!r.store.nodes()[0].stale);
}

#[test]
fn on_swarm_a_worker_that_keeps_talking_does_not_hide_a_manager_that_went_down() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "manager", "", vec![snapshot(&["h1"])]);
    for t in [5, 10, 15, 20, 25, 30] {
        r.svc.handle_at(
            r.at(t),
            &src,
            "worker",
            "",
            vec![Event::Metrics {
                nodes: HashMap::new(),
                edges: HashMap::new(),
            }],
        );
    }
    r.svc.check(r.at(30));
    assert_eq!(
        r.state("s").0,
        "error",
        "the manager is the only one that sees services and tasks"
    );
}

#[test]
fn a_source_none_of_whose_agents_described_the_cluster_yet_is_judged_by_all_of_them() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc.handle_at(r.at(0), &src, "w1", "", vec![]);
    r.svc.check(r.at(25));
    assert_eq!(
        r.state("s"),
        ("error".into(), "agent silent for 25s".into())
    );
}

#[test]
fn a_collector_report_is_the_clusters_state_only_when_it_comes_from_the_agent_that_describes_it() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "manager", "", vec![snapshot(&["h1"])]);
    let report = |state: &str| Event::Report {
        state: state.into(),
        info: format!("{state}!"),
    };

    r.svc
        .handle_at(r.at(1), &src, "worker", "", vec![report("error")]);
    assert_eq!(r.state("s").0, "connected", "a worker's trouble is its own");

    r.svc
        .handle_at(r.at(2), &src, "manager", "", vec![report("error")]);
    assert_eq!(r.state("s"), ("error".into(), "error!".into()));
    assert!(r.store.nodes()[0].stale);

    r.svc
        .handle_at(r.at(3), &src, "manager", "", vec![report("connected")]);
    assert_eq!(r.state("s").0, "connected");
    assert!(!r.store.nodes()[0].stale);
}

#[test]
fn an_agent_that_started_while_the_api_was_down_can_report_it() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc.handle_at(
        r.at(0),
        &src,
        "a1",
        "",
        vec![Event::Report {
            state: "error".into(),
            info: "cannot reach the API".into(),
        }],
    );
    assert_eq!(
        r.state("s"),
        ("error".into(), "cannot reach the API".into())
    );
}

#[test]
fn a_source_that_used_to_be_connected_is_declared_down_when_no_agent_shows_up_after_a_restart() {
    let r = rig();
    r.source("s", "connected");
    r.svc.check(r.at(10));
    assert_eq!(
        r.state("s").0,
        "connected",
        "the agents get time to reconnect"
    );
    r.svc.check(r.at(31));
    assert_eq!(
        r.state("s"),
        (
            "error".into(),
            "no agent has reported since the hub started".into()
        )
    );
    assert_eq!(
        r.store.nodes()[0].kind,
        "cluster",
        "a placeholder makes it visible instead of leaving nothing"
    );
    assert!(r.store.nodes()[0].stale);
}

#[test]
fn a_source_still_waiting_for_its_first_agent_is_not_an_outage() {
    let r = rig();
    r.source("s", "pending");
    r.svc.check(r.at(300));
    assert_eq!(r.state("s").0, "pending");
}

#[test]
fn a_hosts_own_agent_says_whether_the_host_is_alive() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc.handle_at(
        r.at(0),
        &src,
        "manager",
        "h-mgr",
        vec![snapshot(&["h-mgr", "h-w1"])],
    );
    r.svc.handle_at(r.at(0), &src, "worker", "h-w1", vec![]);
    for t in [5, 10, 15, 20, 25, 30] {
        r.svc.handle_at(r.at(t), &src, "manager", "h-mgr", vec![]);
    }
    r.svc.check(r.at(30)); // the worker's agent went quiet at 0: the host is stale, whatever the manager says
    let stale = |id: &str| {
        r.store
            .nodes()
            .into_iter()
            .find(|n| n.id == id)
            .unwrap()
            .stale
    };
    assert_eq!((stale("h-mgr"), stale("h-w1")), (false, true));

    r.svc.handle_at(r.at(31), &src, "worker", "h-w1", vec![]);
    r.svc.check(r.at(32));
    assert!(!stale("h-w1"));
}

#[test]
fn with_the_control_plane_down_a_live_hosts_own_view_says_what_still_runs() {
    let r = rig();
    let src = r.source("s", "pending");
    let mut nodes = vec![
        node("h1", "host", None),
        node("p1", "workload", Some("h1")),
        node("p2", "workload", Some("h1")),
    ];
    nodes[0].own = "ok".into();
    r.svc.handle_at(
        r.at(0),
        &src,
        "cp",
        "",
        vec![Event::Snapshot {
            nodes,
            edges: vec![],
        }],
    );
    r.svc.handle_at(
        r.at(0),
        &src,
        "node1",
        "h1",
        vec![Event::Alive(vec!["p1".into(), "p2".into()])],
    );
    r.svc.handle_at(
        r.at(1),
        &src,
        "cp",
        "",
        vec![Event::Report {
            state: "error".into(),
            info: "api down".into(),
        }],
    );
    let stale = |id: &str| {
        r.store
            .nodes()
            .into_iter()
            .find(|n| n.id == id)
            .unwrap()
            .stale
    };
    assert!(
        stale("p1") && stale("p2"),
        "the source is down: everything is stale"
    );

    r.svc.handle_at(
        r.at(5),
        &src,
        "node1",
        "h1",
        vec![Event::Alive(vec!["p1".into(), "p2".into()])],
    );
    let mut rx = r.store.subscribe();
    r.svc.check(r.at(6));
    assert!(
        !stale("p1") && !stale("p2") && !stale("h1"),
        "the live host vouches for what it runs"
    );
    assert!(rx.try_recv().is_ok(), "one node came back");
    assert!(
        matches!(rx.try_recv(), Err(TryRecvError::Empty)),
        "h1, p1 and p2 all flipped in the same check(): one snapshot for the three, not three"
    );

    r.svc.handle_at(
        r.at(10),
        &src,
        "node1",
        "h1",
        vec![Event::Alive(vec!["p1".into()])],
    );
    r.svc.check(r.at(11));
    assert!(
        !stale("p1") && stale("p2"),
        "what the host stopped vouching for is unknown again"
    );
}

#[test]
fn a_worker_adds_its_own_volumes_to_the_topology_the_manager_describes() {
    let r = rig();
    let src = r.source("s", "pending");
    let volume = Node {
        id: "s:v:w1:data".into(),
        kind: "volume".into(),
        name: "data".into(),
        parent: Some("h-w1".into()),
        ..Default::default()
    };
    // the worker speaks first: its volumes wait for the topology
    r.svc.handle_at(
        r.at(0),
        &src,
        "worker-a",
        "h-w1",
        vec![Event::Contribution(vec![volume.clone()], vec![])],
    );
    assert!(r.store.nodes().is_empty());
    r.svc.handle_at(
        r.at(1),
        &src,
        "manager",
        "h-mgr",
        vec![snapshot(&["h-mgr", "h-w1"])],
    );
    assert_eq!(
        r.store
            .nodes()
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["h-mgr", "h-w1", "s:v:w1:data"]
    );

    // the worker's container is replaced: the new instance says the same host, so it replaces, not duplicates
    r.svc.handle_at(
        r.at(2),
        &src,
        "worker-b",
        "h-w1",
        vec![Event::Contribution(vec![volume], vec![])],
    );
    assert_eq!(r.store.nodes().len(), 3);
    assert_eq!(r.state("s").0, "connected");
}

#[test]
fn a_contribution_does_not_make_its_agent_the_one_that_describes_the_cluster() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "manager", "", vec![snapshot(&["h1"])]);
    for t in [5, 10, 15, 20, 25, 30] {
        r.svc.handle_at(
            r.at(t),
            &src,
            "worker",
            "h-w1",
            vec![Event::Contribution(vec![], vec![])],
        );
    }
    r.svc.check(r.at(30));
    assert_eq!(
        r.state("s").0,
        "error",
        "the manager went quiet, whatever the worker sends"
    );
}

#[test]
fn a_second_source_for_the_same_cluster_is_a_duplicate_and_is_not_taken_down_by_it() {
    let r = rig();
    let a = r.source("a", "pending");
    let b = r.source("b", "pending");
    let with_uid = |id: &str| {
        let mut n = node(id, "cluster", None);
        n.meta.insert("uid".into(), json!("same"));
        Event::Snapshot {
            nodes: vec![n],
            edges: vec![],
        }
    };
    r.svc.handle_at(r.at(0), &a, "x", "", vec![with_uid("a")]);
    r.svc.handle_at(r.at(0), &b, "y", "", vec![with_uid("b")]);
    assert_eq!(r.state("b").0, "duplicate");
    assert_eq!(r.store.nodes().len(), 1);
}

#[test]
fn protobuf_events_become_ingest_events_and_empty_ones_are_skipped() {
    let proto = pb::Event {
        kind: Some(pb::event::Kind::Status(pb::Status {
            id: "n".into(),
            own: pb::Own::Crit.into(),
            reason: "OOM".into(),
        })),
    };
    assert_eq!(
        Event::from_proto(proto),
        Some(Event::Status {
            id: "n".into(),
            own: "crit".into(),
            reason: "OOM".into()
        })
    );
    assert_eq!(
        Event::from_proto(pb::Event { kind: None }),
        None,
        "a newer agent may send what this hub does not know"
    );
}

#[test]
fn a_hello_records_what_the_agent_says_about_itself() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc.hello_at(
        r.at(0),
        &src,
        "pod-b",
        "h2",
        HelloInfo {
            version: "1.2.0",
            collector: "node",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    r.svc.hello_at(
        r.at(1),
        &src,
        "pod-a",
        "",
        HelloInfo {
            version: "0.9.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    r.svc.handle_at(r.at(5), &src, "pod-a", "", vec![]);
    let agents = r.svc.agents_at(r.at(8), "s");
    assert_eq!(
        agents
            .iter()
            .map(|a| (
                a.id.as_str(),
                a.version.as_str(),
                a.collector.as_str(),
                a.host.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("pod-a", "0.9.0", "kubernetes", ""),
            ("pod-b", "1.2.0", "node", "h2")
        ]
    );
    assert_eq!(
        (agents[0].seen_ago, agents[1].seen_ago),
        (Duration::from_secs(3), Duration::from_secs(8)),
        "heard from at the last batch, and at the hello"
    );
    assert!(r.svc.agents_at(r.at(8), "other").is_empty());

    r.svc.hello_at(
        r.at(9),
        &src,
        "pod-a",
        "",
        HelloInfo {
            version: "1.0.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    assert_eq!(
        r.svc.agents_at(r.at(9), "s")[0].version,
        "1.0.0",
        "an agent that was upgraded and reconnected says so"
    );
}

/// The hub stays silently backward compatible with an agent on an older wire protocol — this only confirms the number it reported is
/// kept for the admin to see, which is what a "protocol prea vechi" warning (TODO 6a) is built on.
#[test]
fn a_hello_s_protocol_number_is_recorded_as_reported() {
    let r = rig();
    let src = r.source("s", "pending");
    r.svc.hello_at(
        r.at(0),
        &src,
        "pod-old",
        "",
        HelloInfo {
            version: "1.0.0",
            collector: "kubernetes",
            protocol: 0,
        },
    );
    r.svc.hello_at(
        r.at(0),
        &src,
        "pod-current",
        "",
        HelloInfo {
            version: "1.0.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    let agents = r.svc.agents_at(r.at(0), "s");
    assert_eq!(
        agents
            .iter()
            .map(|a| (a.id.as_str(), a.protocol))
            .collect::<Vec<_>>(),
        [("pod-current", hermes_proto::PROTOCOL), ("pod-old", 0)]
    );
    assert!(agents.iter().find(|a| a.id == "pod-old").unwrap().protocol < hermes_proto::PROTOCOL);
}

#[test]
fn a_hello_that_changes_what_the_admin_sees_tells_the_browser() {
    let r = rig();
    let src = r.source("s", "pending");
    let mut rx = r.store.subscribe();

    r.svc.hello_at(
        r.at(0),
        &src,
        "pod-a",
        "h1",
        HelloInfo {
            version: "1.0.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    assert!(rx.try_recv().is_ok(), "a new agent shows up live");

    r.svc.hello_at(
        r.at(1),
        &src,
        "pod-a",
        "h1",
        HelloInfo {
            version: "1.0.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    assert!(
        matches!(rx.try_recv(), Err(TryRecvError::Empty)),
        "the same hello again says nothing new: no event"
    );

    r.svc.hello_at(
        r.at(2),
        &src,
        "pod-a",
        "h1",
        HelloInfo {
            version: "1.1.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    assert!(rx.try_recv().is_ok(), "an upgraded version shows up live");

    r.svc.hello_at(
        r.at(3),
        &src,
        "pod-a",
        "h2",
        HelloInfo {
            version: "1.1.0",
            collector: "kubernetes",
            protocol: hermes_proto::PROTOCOL,
        },
    );
    assert!(
        rx.try_recv().is_ok(),
        "the same agent back on a different host (rescheduled) shows up live too"
    );
}

#[test]
fn durations_read_like_the_ones_of_the_go_hub() {
    let f = |s| fmt_duration(Duration::from_secs(s));
    assert_eq!(
        (
            f(25).as_str(),
            f(65).as_str(),
            f(120).as_str(),
            f(7200).as_str()
        ),
        ("25s", "1m5s", "2m0s", "2h0m0s")
    );
}

fn machine(id: &str) -> Vec<Event> {
    vec![
        Event::Report {
            state: "connected".into(),
            info: format!("{id} · 3 containers"),
        },
        Event::Contribution(vec![node(&format!("s:n:{id}"), "host", Some("s"))], vec![]),
    ]
}

#[test]
fn a_machine_that_reports_itself_is_not_a_source_nobody_describes() {
    let r = rig();
    let src = r.docker_source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "vm-1", "s:n:vm1", machine("vm1"));
    for t in [10, 20, 30, 40] {
        r.svc.handle_at(r.at(t), &src, "vm-1", "s:n:vm1", vec![]);
    }
    r.svc.check(r.at(40));
    assert_eq!(
        r.state("s").0,
        "connected",
        "no snapshot ever comes, and none is needed"
    );
    let nodes = r.store.nodes();
    assert_eq!(nodes.iter().filter(|n| n.kind == "host").count(), 1);
    assert!(nodes.iter().all(|n| !n.stale));
}

#[test]
fn a_docker_source_is_one_machine_and_turns_another_one_away_while_the_first_is_around() {
    let r = rig();
    let src = r.docker_source("s", "pending");
    let info = |v| HelloInfo {
        version: v,
        collector: "docker",
        protocol: 1,
    };
    r.svc
        .hello_at(r.at(0), &src, "agent-a", "s:n:vm1", info("1.0.0"));
    assert!(r.svc.admit_at(r.at(1), &src, "agent-a", "s:n:vm1").is_ok());
    // the same machine with a new agent (a restart gives it a new id) is the same machine
    assert!(r.svc.admit_at(r.at(2), &src, "agent-a2", "s:n:vm1").is_ok());
    let why = r
        .svc
        .admit_at(r.at(2), &src, "agent-b", "s:n:vm2")
        .unwrap_err();
    assert!(why.contains("vm1") && why.contains("add a source"), "{why}");
    // it was replaced: nothing from the first for longer than that
    assert!(
        r.svc
            .admit_at(r.at(400), &src, "agent-b", "s:n:vm2")
            .is_ok()
    );
}

#[test]
fn other_sources_take_any_agent_that_has_the_token() {
    let r = rig();
    let src = r.source("s", "pending"); // a swarm: one agent on every node
    let info = |v| HelloInfo {
        version: v,
        collector: "swarm",
        protocol: 1,
    };
    r.svc
        .hello_at(r.at(0), &src, "agent-a", "s:n:node1", info("1.0.0"));
    assert!(
        r.svc
            .admit_at(r.at(1), &src, "agent-b", "s:n:node2")
            .is_ok()
    );
}

#[test]
fn a_machine_that_takes_the_place_of_another_leaves_nothing_of_it_on_the_map() {
    let r = rig();
    let src = r.docker_source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "vm-1", "s:n:vm1", machine("vm1"));
    r.svc
        .handle_at(r.at(400), &src, "vm-2", "s:n:vm2", machine("vm2"));
    let hosts: Vec<String> = r
        .store
        .nodes()
        .into_iter()
        .filter(|n| n.kind == "host")
        .map(|n| n.id)
        .collect();
    assert_eq!(hosts, ["s:n:vm2"]);
}

#[test]
fn when_every_machine_goes_quiet_the_docker_source_is_down() {
    let r = rig();
    let src = r.docker_source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "vm-1", "s:n:vm1", machine("vm1"));
    r.svc.check(r.at(60));
    assert_eq!(r.state("s").0, "error");
}

#[test]
fn an_agent_that_still_talks_after_its_source_was_removed_does_not_bring_the_cluster_back() {
    let r = rig();
    let src = r.docker_source("s", "pending");
    r.svc
        .handle_at(r.at(0), &src, "vm-1", "s:n:vm1", machine("vm1"));
    assert_eq!(
        r.store
            .nodes()
            .iter()
            .filter(|n| n.kind == "cluster")
            .count(),
        1
    );

    // the admin removes the source: the store and the database forget it, but the agent's stream stays up until it is checked again
    r.db.delete_source("s").unwrap();
    r.store.remove_source("s");
    assert!(r.store.nodes().is_empty());

    r.svc
        .handle_at(r.at(5), &src, "vm-1", "s:n:vm1", machine("vm1"));
    r.svc
        .handle_at(r.at(6), &src, "manager", "", vec![snapshot(&["h1"])]);
    assert!(
        r.store.nodes().is_empty(),
        "nothing of a removed source is drawn again: {:?}",
        r.store.nodes()
    );
}

#[test]
fn a_source_that_was_renamed_keeps_the_new_name_for_its_cluster_whatever_the_agent_says() {
    let r = rig();
    let src = r.source("s", "pending"); // the agent was installed with the name of the first version
    r.svc
        .handle_at(r.at(0), &src, "manager", "", vec![snapshot(&["h1"])]);
    let mut cluster = node("s", "cluster", None);
    cluster.name = "old name".into();
    r.svc.handle_at(
        r.at(1),
        &src,
        "manager",
        "",
        vec![Event::Snapshot {
            nodes: vec![cluster.clone()],
            edges: vec![],
        }],
    );
    assert_eq!(
        r.store
            .nodes()
            .iter()
            .find(|n| n.kind == "cluster")
            .unwrap()
            .name,
        "name-s"
    );

    // renamed in the database and in the store, as the controller does; the agent still sends its old name in the next snapshot
    r.db.rename_source("s", "renamed").unwrap();
    r.store.rename_cluster("s", "renamed");
    assert_eq!(
        r.store
            .nodes()
            .iter()
            .find(|n| n.kind == "cluster")
            .unwrap()
            .name,
        "renamed"
    );
    r.svc.handle_at(
        r.at(2),
        &src,
        "manager",
        "",
        vec![Event::Snapshot {
            nodes: vec![cluster],
            edges: vec![],
        }],
    );
    assert_eq!(
        r.store
            .nodes()
            .iter()
            .find(|n| n.kind == "cluster")
            .unwrap()
            .name,
        "renamed"
    );
}
