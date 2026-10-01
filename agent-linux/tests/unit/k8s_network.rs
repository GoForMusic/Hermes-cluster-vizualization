use hermes_proto::value::json_from_struct;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;

fn from<T: DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn service(ns: &str, name: &str, typ: &str, extra: Value) -> Service {
    let mut spec =
        json!({"type": typ, "clusterIP": "10.0.0.7", "ports": [{"port": 80}, {"port": 443}]});
    spec.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    from(json!({"metadata": {"name": name, "namespace": ns}, "spec": spec}))
}

fn slice(ns: &str, service: &str, pods: &[&str]) -> EndpointSlice {
    let endpoints: Vec<Value> = pods
        .iter()
        .map(|p| json!({"addresses": ["10.1.0.1"], "targetRef": {"kind": "Pod", "name": p, "namespace": ns}}))
        .collect();
    from(
        json!({"metadata": {"name": format!("{service}-abc"), "namespace": ns, "labels": {"kubernetes.io/service-name": service}}, "addressType": "IPv4", "endpoints": endpoints}),
    )
}

fn ingress(ns: &str, name: &str, host: &str, service: &str) -> Ingress {
    from(
        json!({"metadata": {"name": name, "namespace": ns}, "spec": {"rules": [{"host": host, "http": {"paths": [{"path": "/", "pathType": "Prefix", "backend": {"service": {"name": service, "port": {"number": 80}}}}]}}]}}),
    )
}

fn pods(names: &[&str]) -> HashSet<String> {
    names.iter().map(|n| format!("s1:p:demo:{n}")).collect()
}

fn run(
    services: &[Service],
    slices: &[EndpointSlice],
    ingresses: &[Ingress],
    pod_ids: &HashSet<String>,
) -> (Vec<PbNode>, Vec<Edge>) {
    run_all(services, slices, ingresses, &[], &[], &[], &[], pod_ids)
}

#[allow(clippy::too_many_arguments)]
fn run_all(
    services: &[Service],
    slices: &[EndpointSlice],
    ingresses: &[Ingress],
    gateways: &[DynamicObject],
    routes: &[DynamicObject],
    policies: &[NetworkPolicy],
    pods: &[Pod],
    pod_ids: &HashSet<String>,
) -> (Vec<PbNode>, Vec<Edge>) {
    build(&Inputs {
        source_id: "s1",
        now_ms: 7,
        services,
        slices,
        ingresses,
        gateways,
        routes,
        policies,
        pods,
        pod_ids,
    })
}

fn gateway(ns: &str, name: &str, address: Option<&str>) -> DynamicObject {
    let mut v = json!({"apiVersion": "gateway.networking.k8s.io/v1", "kind": "Gateway", "metadata": {"name": name, "namespace": ns}, "spec": {"gatewayClassName": "traefik", "listeners": [{"name": "web", "port": 80, "protocol": "HTTP"}]}});
    if let Some(a) = address {
        v["status"] = json!({"addresses": [{"type": "IPAddress", "value": a}]});
    }
    from(v)
}

fn http_route(ns: &str, name: &str, gateway: &str, service: &str) -> DynamicObject {
    from(
        json!({"apiVersion": "gateway.networking.k8s.io/v1", "kind": "HTTPRoute", "metadata": {"name": name, "namespace": ns},
        "spec": {"parentRefs": [{"name": gateway}], "hostnames": ["shop.example.com"], "rules": [{"backendRefs": [{"name": service, "port": 80}]}]}}),
    )
}

fn pod_with(ns: &str, name: &str, labels: Value) -> Pod {
    from(json!({"metadata": {"name": name, "namespace": ns, "labels": labels}}))
}

fn policy(ns: &str, name: &str, spec: Value) -> NetworkPolicy {
    from(json!({"metadata": {"name": name, "namespace": ns}, "spec": spec}))
}

fn meta(n: &PbNode) -> Value {
    json_from_struct(n.meta.as_ref().unwrap())
}

fn routes(edges: &[Edge]) -> Vec<(&str, &str)> {
    edges
        .iter()
        .map(|e| (e.from.as_str(), e.to.as_str()))
        .collect()
}

#[test]
fn a_service_is_a_network_node_with_a_route_to_each_pod_behind_it() {
    let (nodes, edges) = run(
        &[service("demo", "web", "ClusterIP", json!({}))],
        &[slice("demo", "web", &["web-1", "web-2"])],
        &[],
        &pods(&["web-1", "web-2"]),
    );
    assert_eq!(nodes.len(), 1);
    assert_eq!(
        (
            nodes[0].id.as_str(),
            nodes[0].kind(),
            nodes[0].parent.as_deref()
        ),
        ("s1:svc:demo:web", NodeKind::Network, Some("s1"))
    );
    assert_eq!(
        meta(&nodes[0]),
        json!({"type": "Service (ClusterIP)", "netKind": "service", "ns": "demo", "addr": "10.0.0.7:80,443", "members": 2, "pending": false, "ip": "10.0.0.7"})
    );
    assert_eq!(
        routes(&edges),
        [
            ("s1:svc:demo:web", "s1:p:demo:web-1"),
            ("s1:svc:demo:web", "s1:p:demo:web-2")
        ]
    );
    assert!(edges.iter().all(|e| e.r#type() == EdgeType::Route));
}

#[test]
fn pods_that_are_not_drawn_and_services_with_nothing_behind_them_are_left_out() {
    let (nodes, edges) = run(
        &[
            service("demo", "web", "ClusterIP", json!({})),
            service("default", "kubernetes", "ClusterIP", json!({})),
            service("demo", "db", "ExternalName", json!({})),
        ],
        &[slice("demo", "web", &["web-1", "gone"])],
        &[],
        &pods(&["web-1"]),
    );
    assert_eq!(
        nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["web"]
    );
    assert_eq!(routes(&edges), [("s1:svc:demo:web", "s1:p:demo:web-1")]);
}

#[test]
fn an_ingress_sends_to_its_services_and_outside_reaches_the_ingress() {
    let (nodes, edges) = run(
        &[service("demo", "web", "ClusterIP", json!({}))],
        &[slice("demo", "web", &["web-1"])],
        &[ingress("demo", "site", "shop.example.com", "web")],
        &pods(&["web-1"]),
    );
    assert_eq!(
        nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["s1:outside", "s1:ing:demo:site", "s1:svc:demo:web"]
    );
    assert_eq!(meta(&nodes[1])["addr"], "shop.example.com");
    assert_eq!(meta(&nodes[1])["netKind"], "ingress");
    assert_eq!(
        routes(&edges),
        [
            ("s1:svc:demo:web", "s1:p:demo:web-1"),
            ("s1:ing:demo:site", "s1:svc:demo:web"),
            ("s1:outside", "s1:ing:demo:site")
        ]
    );
}

#[test]
fn a_load_balancer_without_an_address_says_so_and_is_reached_from_outside() {
    let (nodes, edges) = run(
        &[service("demo", "web-lb", "LoadBalancer", json!({}))],
        &[slice("demo", "web-lb", &["web-1"])],
        &[],
        &pods(&["web-1"]),
    );
    let lb = nodes.iter().find(|n| n.name == "web-lb").unwrap();
    assert_eq!(
        (
            meta(lb)["netKind"].clone(),
            meta(lb)["addr"].clone(),
            meta(lb)["pending"].clone()
        ),
        (json!("loadbalancer"), json!("no address yet"), json!(true))
    );
    assert!(routes(&edges).contains(&("s1:outside", "s1:svc:demo:web-lb")));
}

#[test]
fn a_load_balancer_with_an_address_shows_it() {
    let mut lb = service("demo", "web-lb", "LoadBalancer", json!({}));
    lb.status = Some(from(
        json!({"loadBalancer": {"ingress": [{"ip": "192.168.150.200"}]}}),
    ));
    let (nodes, _) = run(
        &[lb],
        &[slice("demo", "web-lb", &["web-1"])],
        &[],
        &pods(&["web-1"]),
    );
    assert_eq!(
        meta(nodes.iter().find(|n| n.name == "web-lb").unwrap())["addr"],
        "192.168.150.200"
    );
}

#[test]
fn a_service_only_an_ingress_sends_to_is_drawn_even_with_no_pods_yet_and_a_link_to_a_service_that_is_not_there_is_not()
 {
    let (nodes, edges) = run(
        &[service("demo", "web", "ClusterIP", json!({}))],
        &[],
        &[
            ingress("demo", "site", "a.example", "web"),
            ingress("demo", "other", "b.example", "missing"),
        ],
        &pods(&[]),
    );
    assert!(nodes.iter().any(|n| n.name == "web"));
    assert!(routes(&edges).contains(&("s1:ing:demo:site", "s1:svc:demo:web")));
    assert!(!routes(&edges).iter().any(|(_, to)| to.contains("missing")));
}

#[test]
fn a_headless_service_says_so() {
    let (nodes, _) = run(
        &[service(
            "demo",
            "db",
            "ClusterIP",
            json!({"clusterIP": "None", "ports": [{"port": 5432}]}),
        )],
        &[slice("demo", "db", &["db-0"])],
        &[],
        &pods(&["db-0"]),
    );
    assert_eq!(meta(&nodes[0])["addr"], "headless:5432");
}

#[test]
fn a_gateway_api_route_leads_from_its_gateway_to_its_services_and_the_gateway_from_outside() {
    let (nodes, edges) = run_all(
        &[service("demo", "web", "ClusterIP", json!({}))],
        &[slice("demo", "web", &["web-1"])],
        &[],
        &[gateway("demo", "edge", Some("192.168.1.9"))],
        &[http_route("demo", "shop", "edge", "web")],
        &[],
        &[],
        &pods(&["web-1"]),
    );
    assert_eq!(
        nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        [
            "s1:outside",
            "s1:gw:demo:edge",
            "s1:rt:demo:shop",
            "s1:svc:demo:web"
        ]
    );
    assert_eq!(
        meta(&nodes[1]),
        json!({"type": "Gateway", "netKind": "gateway", "ns": "demo", "addr": "192.168.1.9:80", "members": 1, "pending": false})
    );
    assert_eq!(meta(&nodes[2])["addr"], "shop.example.com");
    assert_eq!(
        routes(&edges),
        [
            ("s1:svc:demo:web", "s1:p:demo:web-1"),
            ("s1:rt:demo:shop", "s1:svc:demo:web"),
            ("s1:gw:demo:edge", "s1:rt:demo:shop"),
            ("s1:outside", "s1:gw:demo:edge"),
        ]
    );
}

#[test]
fn a_gateway_without_an_address_says_so() {
    let (nodes, _) = run_all(
        &[],
        &[],
        &[],
        &[gateway("demo", "edge", None)],
        &[],
        &[],
        &[],
        &pods(&[]),
    );
    let gw = nodes.iter().find(|n| n.name == "edge").unwrap();
    assert_eq!(
        (meta(gw)["addr"].clone(), meta(gw)["pending"].clone()),
        (json!("no address yet · 80"), json!(true))
    );
}

#[test]
fn a_route_to_something_other_than_a_service_or_from_something_other_than_a_gateway_is_ignored() {
    let mut odd = http_route("demo", "odd", "edge", "web");
    odd.data["spec"]["rules"][0]["backendRefs"][0]["kind"] = json!("Bucket");
    odd.data["spec"]["parentRefs"][0]["kind"] = json!("Mesh");
    assert!(route_backends(&odd).is_empty() && route_parents(&odd).is_empty());
    let other_ns = {
        let mut r = http_route("apps", "x", "edge", "web");
        r.data["spec"]["parentRefs"][0]["namespace"] = json!("infra");
        r
    };
    assert_eq!(
        (route_parents(&other_ns), route_backends(&other_ns)),
        (vec!["infra/edge".to_string()], vec!["apps/web".to_string()])
    );
}

#[test]
fn a_network_policy_leads_to_the_pods_it_applies_to_and_says_what_it_allows() {
    let pods_all = [
        pod_with("demo", "web-1", json!({"app": "web"})),
        pod_with("demo", "web-2", json!({"app": "web"})),
        pod_with("demo", "db-0", json!({"app": "db"})),
        pod_with("other", "web-9", json!({"app": "web"})),
    ];
    let ids = pods(&["web-1", "web-2", "db-0"]);
    let (nodes, edges) = run_all(
        &[],
        &[],
        &[],
        &[],
        &[],
        &[
            policy(
                "demo",
                "web-open",
                json!({"podSelector": {"matchLabels": {"app": "web"}}, "ingress": [{}]}),
            ),
            policy(
                "demo",
                "db-lock",
                json!({"podSelector": {"matchExpressions": [{"key": "app", "operator": "In", "values": ["db"]}]}, "policyTypes": ["Ingress", "Egress"], "egress": [{}]}),
            ),
            policy("demo", "deny-all", json!({"podSelector": {}})),
            policy(
                "demo",
                "nobody",
                json!({"podSelector": {"matchLabels": {"app": "none"}}}),
            ),
        ],
        &pods_all,
        &ids,
    );
    let by_name = |n: &str| nodes.iter().find(|x| x.name == n);
    assert_eq!(meta(by_name("web-open").unwrap())["addr"], "ingress");
    assert_eq!(meta(by_name("web-open").unwrap())["members"], 2);
    assert_eq!(
        meta(by_name("db-lock").unwrap())["addr"],
        "deny ingress · egress"
    );
    assert_eq!(
        (
            meta(by_name("deny-all").unwrap())["addr"].clone(),
            meta(by_name("deny-all").unwrap())["members"].clone()
        ),
        (json!("deny ingress"), json!(3))
    );
    assert!(
        by_name("nobody").is_none(),
        "a policy that applies to nothing drawn is not drawn"
    );
    let r = routes(&edges);
    assert!(
        r.contains(&("s1:np:demo:web-open", "s1:p:demo:web-1"))
            && !r.iter().any(|(_, to)| to.contains("web-9"))
    );
}

#[test]
fn selectors_match_the_way_kubernetes_says() {
    let labels: BTreeMap<String, String> = [
        ("app".to_string(), "web".to_string()),
        ("tier".to_string(), "front".to_string()),
    ]
    .into();
    let sel = |v: Value| -> LabelSelector { from(v) };
    assert!(
        selects(&sel(json!({})), &labels),
        "empty selects everything"
    );
    assert!(selects(
        &sel(json!({"matchLabels": {"app": "web"}})),
        &labels
    ));
    assert!(!selects(
        &sel(json!({"matchLabels": {"app": "db"}})),
        &labels
    ));
    assert!(selects(
        &sel(
            json!({"matchExpressions": [{"key": "tier", "operator": "NotIn", "values": ["back"]}]})
        ),
        &labels
    ));
    assert!(selects(
        &sel(
            json!({"matchExpressions": [{"key": "app", "operator": "Exists"}, {"key": "gpu", "operator": "DoesNotExist"}]})
        ),
        &labels
    ));
    assert!(!selects(
        &sel(json!({"matchExpressions": [{"key": "app", "operator": "Bogus"}]})),
        &labels
    ));
}

#[test]
fn a_node_port_is_reached_from_outside_and_a_cluster_with_nothing_of_the_kind_has_no_outside() {
    let (nodes, edges) = run(
        &[service("demo", "np", "NodePort", json!({}))],
        &[slice("demo", "np", &["web-1"])],
        &[],
        &pods(&["web-1"]),
    );
    assert!(routes(&edges).contains(&("s1:outside", "s1:svc:demo:np")));
    assert_eq!(
        meta(nodes.iter().find(|n| n.name == "np").unwrap())["netKind"],
        "nodeport"
    );
    let (nodes, _) = run(
        &[service("demo", "web", "ClusterIP", json!({}))],
        &[slice("demo", "web", &["web-1"])],
        &[],
        &pods(&["web-1"]),
    );
    assert!(nodes.iter().all(|n| n.id != "s1:outside"));
}

#[test]
fn an_ingress_controller_is_recognised_by_its_image_or_its_labels_and_a_web_server_is_not() {
    let pod = |labels: Value, image: &str| -> Pod {
        from(
            json!({"metadata": {"name": "p", "labels": labels}, "spec": {"containers": [{"name": "c", "image": image}]}}),
        )
    };
    assert!(is_ingress_controller(&pod(json!({}), "traefik:v3.1")));
    assert!(is_ingress_controller(&pod(
        json!({"app.kubernetes.io/name": "ingress-nginx"}),
        "registry.k8s.io/ingress-nginx/controller:v1.11"
    )));
    assert!(is_ingress_controller(&pod(json!({"app": "kong"}), "x")));
    assert!(
        !is_ingress_controller(&pod(json!({"app": "web"}), "nginx:alpine")),
        "nginx alone is a web server"
    );
    assert!(!is_ingress_controller(&pod(json!({}), "busybox:1.36")));
}
