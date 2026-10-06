use super::*;
use crate::docker::{ContainerNetwork, ContainerNetworks, SwarmNetwork};

fn container(
    id: &str,
    name: &str,
    state: &str,
    status: &str,
    project: Option<&str>,
) -> ContainerSummary {
    ContainerSummary {
        id: id.into(),
        names: vec![format!("/{name}")],
        image: "nginx:alpine".into(),
        state: state.into(),
        status: status.into(),
        created: 1_700_000_000,
        labels: project.map(|p| HashMap::from([(COMPOSE_PROJECT.to_string(), p.to_string())])),
        network_settings: None,
    }
}

fn info() -> EngineInfo {
    EngineInfo {
        id: "ABCD:EFGH:IJKL:MNOP".into(),
        name: "vm-build-1".into(),
        ncpu: 4,
        mem_total: 8 << 30,
        architecture: "x86_64".into(),
        os_type: "linux".into(),
        ..Default::default()
    }
}

#[test]
fn the_machine_is_told_apart_by_the_engine_id_without_its_colons() {
    assert_eq!(machine_key(&info()), "ABCDEFGHIJKL");
    let none = EngineInfo {
        name: "vm".into(),
        ..Default::default()
    };
    assert_eq!(machine_key(&none), "vm");
}

#[test]
fn a_machine_is_a_host_under_the_source_and_its_containers_are_workloads_on_it() {
    let cs = [
        container(
            "aaaaaaaaaaaa1111",
            "web-1",
            "running",
            "Up 2 hours",
            Some("shop"),
        ),
        container("bbbbbbbbbbbb2222", "db-1", "running", "Up 2 hours", None),
    ];
    let nodes = build("src", &info(), "rack-2", &cs, &[], 5).0;
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0].id, "src:n:ABCDEFGHIJKL");
    assert_eq!(nodes[0].parent.as_deref(), Some("src"));
    assert_eq!(nodes[0].kind(), NodeKind::Host);
    assert!(format!("{:?}", nodes[0].meta).contains("rack-2"));
    // sorted by name: db-1 first
    assert_eq!(nodes[1].name, "db-1");
    assert_eq!(nodes[1].id, "src:c:bbbbbbbbbbbb");
    assert_eq!(nodes[1].parent.as_deref(), Some("src:n:ABCDEFGHIJKL"));
    assert_eq!(nodes[2].provider(), Provider::Docker);
}

#[test]
fn without_a_location_the_host_has_none() {
    let nodes = build("src", &info(), "", &[], &[], 5).0;
    assert!(
        !nodes[0]
            .meta
            .as_ref()
            .unwrap()
            .fields
            .contains_key("location")
    );
}

#[test]
fn a_container_is_judged_by_its_state() {
    let s = |state, status| container_state(&container("x", "x", state, status, None));
    assert_eq!(s("running", "Up 1 hour"), Some((Own::Ok, String::new())));
    assert_eq!(s("running", "Up 1 hour (unhealthy)").unwrap().0, Own::Warn);
    assert_eq!(
        s("restarting", "Restarting (1) 3 seconds ago").unwrap().0,
        Own::Crit
    );
    assert_eq!(
        s("exited", "Exited (137) 2 hours ago"),
        Some((Own::Crit, "exited with code 137".into()))
    );
    assert_eq!(s("paused", "Up 1 hour (Paused)").unwrap().0, Own::Warn);
}

#[test]
fn a_container_that_finished_by_itself_is_not_on_the_map() {
    assert_eq!(
        container_state(&container(
            "x",
            "job",
            "exited",
            "Exited (0) 1 minute ago",
            None
        )),
        None
    );
    let nodes = build(
        "src",
        &info(),
        "",
        &[container(
            "x",
            "job",
            "exited",
            "Exited (0) 1 minute ago",
            None,
        )],
        &[],
        5,
    )
    .0;
    assert_eq!(nodes.len(), 1);
}

fn on(mut c: ContainerSummary, nets: &[(&str, &str)]) -> ContainerSummary {
    let networks = nets
        .iter()
        .map(|(name, id)| (name.to_string(), ContainerNetwork { id: id.to_string() }))
        .collect();
    c.network_settings = Some(ContainerNetworks {
        networks: Some(networks),
    });
    c
}

fn network(id: &str, name: &str, driver: &str) -> SwarmNetwork {
    SwarmNetwork {
        id: id.into(),
        name: name.into(),
        driver: driver.into(),
        ..Default::default()
    }
}

#[test]
fn a_network_you_made_is_drawn_with_the_containers_on_it_and_the_engines_own_are_not() {
    let web = on(
        container("aaaaaaaaaaaa1111", "web-1", "running", "Up", Some("shop")),
        &[("shop_default", "n1n1n1n1n1n1n1n1")],
    );
    let lonely = on(
        container("bbbbbbbbbbbb2222", "db-1", "running", "Up", None),
        &[("bridge", "brbrbrbrbrbr")],
    );
    let nets = [
        network("n1n1n1n1n1n1n1n1", "shop_default", "bridge"),
        network("brbrbrbrbrbr", "bridge", "bridge"),
        network("hohohohohoho", "host", "host"),
        network("emptyemptyempty", "unused", "bridge"),
    ];
    let (nodes, edges) = build("src", &info(), "", &[web, lonely], &nets, 5);
    let drawn: Vec<&str> = nodes
        .iter()
        .filter(|n| n.kind() == NodeKind::Network)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(
        drawn,
        ["shop_default"],
        "not bridge, not host, not an empty one"
    );
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].to, "src:c:aaaaaaaaaaaa");
    assert_eq!(edges[0].from, "src:net:ABCDEFGHIJKL:n1n1n1n1n1n1");
    assert_eq!(edges[0].id, format!("{}>{}", edges[0].from, edges[0].to));
}

#[test]
fn the_same_network_name_on_two_machines_is_two_networks() {
    let mut other = info();
    other.id = "WXYZ:1234:5678".into();
    let c = on(
        container("aaaaaaaaaaaa1111", "web-1", "running", "Up", None),
        &[("shop_default", "n1n1n1n1n1n1")],
    );
    let nets = [network("n1n1n1n1n1n1", "shop_default", "bridge")];
    let a = build("src", &info(), "", &[], &nets, 5).1;
    assert!(a.is_empty(), "nobody on it, nothing drawn");
    let one = build("src", &info(), "", std::slice::from_ref(&c), &nets, 5);
    let two = build("src", &other, "", std::slice::from_ref(&c), &nets, 5);
    assert_ne!(one.1[0].from, two.1[0].from);
}

#[test]
fn only_a_container_on_a_single_network_has_its_traffic_put_on_that_link() {
    let one = on(
        container("aaaaaaaaaaaa1111", "web-1", "running", "Up", None),
        &[("shop_default", "n1n1n1n1n1n1")],
    );
    let two = on(
        container("bbbbbbbbbbbb2222", "api-1", "running", "Up", None),
        &[("shop_default", "n1n1n1n1n1n1"), ("other", "n2n2n2n2n2n2")],
    );
    let nets = [
        network("n1n1n1n1n1n1", "shop_default", "bridge"),
        network("n2n2n2n2n2n2", "other", "bridge"),
    ];
    let links = single_network_links("src", &info(), &[one, two], &nets);
    assert_eq!(
        links.len(),
        1,
        "two interfaces: which is which is not known"
    );
    assert!(links.contains_key("src:c:aaaaaaaaaaaa"));
}
