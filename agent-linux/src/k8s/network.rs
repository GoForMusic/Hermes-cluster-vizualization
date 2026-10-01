//! How traffic reaches the pods, from the API: Services (with the pods behind them, from their EndpointSlices), Ingresses (with the Services they
//! send to) and "outside", the place both are reached from. They are drawn as network nodes joined by route links: the API says a path exists,
//! not how much moves along it. Pure, like the rest of the topology.

use std::collections::{BTreeMap, HashMap, HashSet};

use hermes_proto::v1::{Edge, EdgeType, Node as PbNode, NodeKind, Own, Provider};
use hermes_proto::value::struct_from_json;
use k8s_openapi::api::core::v1::{Pod, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use k8s_openapi::api::networking::v1::{Ingress, IngressBackend, NetworkPolicy};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use kube::api::DynamicObject;
use serde_json::{Value, json};

pub struct Inputs<'a> {
    pub source_id: &'a str,
    pub now_ms: i64,
    pub services: &'a [Service],
    pub slices: &'a [EndpointSlice],
    pub ingresses: &'a [Ingress],
    /// Gateway API: `Gateway` and `HTTPRoute` objects (empty when the CRDs are not installed).
    pub gateways: &'a [DynamicObject],
    pub routes: &'a [DynamicObject],
    pub policies: &'a [NetworkPolicy],
    pub pods: &'a [Pod],
    /// The ids of the pods that are drawn: a route to anything else would end in nothing.
    pub pod_ids: &'a HashSet<String>,
}

fn ns_name(m: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> (&str, &str) {
    (
        m.namespace.as_deref().unwrap_or_default(),
        m.name.as_deref().unwrap_or_default(),
    )
}

/// The pods behind each Service (`ns/name` -> pod ids), from the endpoint slices that name the Service.
fn pods_behind(i: &Inputs<'_>) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for slice in i.slices {
        let (ns, _) = ns_name(&slice.metadata);
        let Some(service) = slice
            .metadata
            .labels
            .as_ref()
            .and_then(|l| l.get("kubernetes.io/service-name"))
        else {
            continue;
        };
        for endpoint in slice.endpoints.iter().flatten() {
            let Some(target) = endpoint
                .target_ref
                .as_ref()
                .filter(|t| t.kind.as_deref() == Some("Pod"))
            else {
                continue;
            };
            let id = format!(
                "{}:p:{ns}:{}",
                i.source_id,
                target.name.as_deref().unwrap_or_default()
            );
            let list = out.entry(format!("{ns}/{service}")).or_default();
            if i.pod_ids.contains(&id) && !list.contains(&id) {
                list.push(id);
            }
        }
    }
    out
}

/// What a Service is: `LoadBalancer` and `NodePort` are also reachable from outside the cluster; everything else (`ClusterIP`,
/// `ExternalName`, or no type at all) is only reachable from inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceKind {
    LoadBalancer,
    NodePort,
    ClusterIp,
}

impl ServiceKind {
    /// The word the map uses for it, in `meta.netKind` — spelled exactly as before this was an enum, since the frontend reads it as
    /// a plain string.
    fn as_str(self) -> &'static str {
        match self {
            Self::LoadBalancer => "loadbalancer",
            Self::NodePort => "nodeport",
            Self::ClusterIp => "service",
        }
    }
}

fn service_kind(s: &Service) -> ServiceKind {
    match s.spec.as_ref().and_then(|s| s.type_.as_deref()) {
        Some("LoadBalancer") => ServiceKind::LoadBalancer,
        Some("NodePort") => ServiceKind::NodePort,
        _ => ServiceKind::ClusterIp,
    }
}

/// Its address: the cluster IP and the ports, or for a load balancer where it is reachable from (empty while it has none).
fn service_address(s: &Service) -> String {
    let spec = s.spec.as_ref();
    let ports = spec
        .and_then(|s| s.ports.as_ref())
        .map(|p| {
            p.iter()
                .map(|p| p.port.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let cluster_ip = spec
        .and_then(|s| s.cluster_ip.as_deref())
        .unwrap_or_default();
    if cluster_ip == "None" {
        return if ports.is_empty() {
            "headless".to_string()
        } else {
            format!("headless:{ports}")
        };
    }
    match (cluster_ip.is_empty(), ports.is_empty()) {
        (true, _) => String::new(),
        (false, true) => cluster_ip.to_string(),
        (false, false) => format!("{cluster_ip}:{ports}"),
    }
}

fn load_balancer_address(s: &Service) -> Option<String> {
    s.status
        .as_ref()?
        .load_balancer
        .as_ref()?
        .ingress
        .as_ref()?
        .iter()
        .find_map(|i| i.ip.clone().or_else(|| i.hostname.clone()))
}

fn backend_service(b: &IngressBackend) -> Option<&str> {
    b.service.as_ref().map(|s| s.name.as_str())
}

/// The Services an Ingress sends to.
fn ingress_targets(ing: &Ingress) -> Vec<&str> {
    let Some(spec) = ing.spec.as_ref() else {
        return vec![];
    };
    let mut out: Vec<&str> = spec
        .rules
        .iter()
        .flatten()
        .filter_map(|r| r.http.as_ref())
        .flat_map(|h| &h.paths)
        .filter_map(|p| backend_service(&p.backend))
        .chain(spec.default_backend.as_ref().and_then(backend_service))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn ingress_hosts(ing: &Ingress) -> String {
    let hosts: Vec<&str> = ing
        .spec
        .iter()
        .flat_map(|s| s.rules.iter().flatten())
        .filter_map(|r| r.host.as_deref())
        .collect();
    if hosts.is_empty() {
        "any host".to_string()
    } else {
        hosts.join(", ")
    }
}

/// The address a Service has of its own (a headless one has none): the hub uses it to tell which Service a connection asked for.
fn cluster_ip(s: &Service) -> Option<&str> {
    s.spec
        .as_ref()
        .and_then(|s| s.cluster_ip.as_deref())
        .filter(|ip| !ip.is_empty() && *ip != "None")
}

fn set_text(node: &mut PbNode, key: &str, value: &str) {
    if let Some(meta) = node.meta.as_mut() {
        meta.fields.insert(
            key.to_string(),
            prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(value.to_string())),
            },
        );
    }
}

/// Is this pod an ingress controller (Traefik, ingress-nginx, HAProxy, Contour, Kong, Emissary)? Decided by the image and the usual labels: a
/// guess, but a good one, and the traffic that goes through it is marked as approximate on the map.
pub fn is_ingress_controller(pod: &Pod) -> bool {
    const KNOWN: [&str; 8] = [
        "traefik",
        "ingress-nginx",
        "nginx-ingress",
        "haproxy-ingress",
        "haproxytech/kubernetes-ingress",
        "contour",
        "kong",
        "emissary",
    ];
    let labels = pod
        .metadata
        .labels
        .iter()
        .flatten()
        .filter(|(k, _)| k.as_str() == "app" || k.as_str() == "app.kubernetes.io/name")
        .map(|(_, v)| v.as_str());
    let images = pod
        .spec
        .iter()
        .flat_map(|s| &s.containers)
        .filter_map(|c| c.image.as_deref());
    labels.chain(images).any(|text| {
        let text = text.to_lowercase();
        KNOWN.iter().any(|k| text.contains(k))
    })
}

/// Does a label selector select these labels? An empty selector selects everything.
fn selects(selector: &LabelSelector, labels: &BTreeMap<String, String>) -> bool {
    let equal = selector
        .match_labels
        .iter()
        .flatten()
        .all(|(k, v)| labels.get(k) == Some(v));
    let expressions = selector.match_expressions.iter().flatten().all(|e| {
        let value = labels.get(&e.key);
        let among = value.is_some_and(|v| e.values.iter().flatten().any(|x| x == v));
        match e.operator.as_str() {
            "In" => among,
            "NotIn" => !among,
            "Exists" => value.is_some(),
            "DoesNotExist" => value.is_none(),
            _ => false,
        }
    });
    equal && expressions
}

/// What a NetworkPolicy does, in a few words: the directions it covers, and which of them let nothing in (or out).
fn policy_summary(p: &NetworkPolicy) -> String {
    let Some(spec) = p.spec.as_ref() else {
        return String::new();
    };
    let types: Vec<&str> = match spec.policy_types.as_ref() {
        Some(t) if !t.is_empty() => t.iter().map(String::as_str).collect(),
        _ if spec.egress.is_some() => vec!["Ingress", "Egress"],
        _ => vec!["Ingress"],
    };
    types
        .iter()
        .map(|t| {
            let none = if *t == "Egress" {
                spec.egress.as_ref().is_none_or(Vec::is_empty)
            } else {
                spec.ingress.as_ref().is_none_or(Vec::is_empty)
            };
            let word = t.to_lowercase();
            if none { format!("deny {word}") } else { word }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// `ns/name` of the Gateways a route attaches to (a parent that is not a Gateway is left out).
fn route_parents(route: &DynamicObject) -> Vec<String> {
    let ns = route.metadata.namespace.as_deref().unwrap_or_default();
    route.data["spec"]["parentRefs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| text(p, "kind").unwrap_or("Gateway") == "Gateway")
        .filter_map(|p| {
            Some(format!(
                "{}/{}",
                text(p, "namespace").unwrap_or(ns),
                text(p, "name")?
            ))
        })
        .collect()
}

/// `ns/name` of the Services a route sends to.
fn route_backends(route: &DynamicObject) -> Vec<String> {
    let ns = route.metadata.namespace.as_deref().unwrap_or_default();
    let mut out: Vec<String> = route.data["spec"]["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|r| r["backendRefs"].as_array().into_iter().flatten())
        .filter(|b| {
            text(b, "kind").unwrap_or("Service") == "Service"
                && text(b, "group").unwrap_or_default().is_empty()
        })
        .filter_map(|b| {
            Some(format!(
                "{}/{}",
                text(b, "namespace").unwrap_or(ns),
                text(b, "name")?
            ))
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn route_hosts(route: &DynamicObject) -> String {
    let hosts: Vec<&str> = route.data["spec"]["hostnames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if hosts.is_empty() {
        "any host".to_string()
    } else {
        hosts.join(", ")
    }
}

fn gateway_address(gw: &DynamicObject) -> String {
    let status = gw.data["status"]["addresses"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|a| text(a, "value"));
    let ports: Vec<String> = gw.data["spec"]["listeners"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["port"].as_u64())
        .map(|p| p.to_string())
        .collect();
    match (status, ports.is_empty()) {
        (Some(a), false) => format!("{a}:{}", ports.join(",")),
        (Some(a), true) => a.to_string(),
        (None, false) => format!("no address yet · {}", ports.join(",")),
        (None, true) => "no address yet".to_string(),
    }
}

fn svc_id(cid: &str, key: &str) -> String {
    format!("{cid}:svc:{}", key.replace('/', ":"))
}

fn ing_id(cid: &str, key: &str) -> String {
    format!("{cid}:ing:{}", key.replace('/', ":"))
}

fn gw_id(cid: &str, key: &str) -> String {
    format!("{cid}:gw:{}", key.replace('/', ":"))
}

fn route_edge(from: &str, to: &str) -> Edge {
    Edge {
        id: format!("{from}>{to}"),
        from: from.to_string(),
        to: to.to_string(),
        r#type: EdgeType::Route.into(),
        ..Default::default()
    }
}

#[allow(clippy::too_many_arguments)] // one shape shared by every kind of network node this file draws
fn network_node(
    cid: &str,
    now_ms: i64,
    id: String,
    name: &str,
    ns: &str,
    typ: &str,
    kind: &str,
    addr: String,
    members: usize,
    pending: bool,
) -> PbNode {
    PbNode {
        id,
        kind: NodeKind::Network.into(),
        name: name.to_string(),
        parent: Some(cid.to_string()),
        provider: Provider::Kubernetes.into(),
        own: Own::Ok.into(),
        since: now_ms,
        meta: Some(struct_from_json(json!({
            "type": typ, "netKind": kind, "ns": ns, "addr": addr, "members": members, "pending": pending,
        }))),
        ..Default::default()
    }
}

/// Service nodes, each with a route to every pod behind it.
fn build_services(
    cid: &str,
    now_ms: i64,
    drawn: &BTreeMap<String, &Service>,
    behind: &HashMap<String, Vec<String>>,
    edges: &mut Vec<Edge>,
) -> Vec<PbNode> {
    let mut svc_nodes = Vec::new();
    for (key, s) in drawn {
        let (ns, name) = ns_name(&s.metadata);
        let kind = service_kind(s);
        let pods = behind.get(key).map(Vec::as_slice).unwrap_or_default();
        let id = svc_id(cid, key);
        let lb = load_balancer_address(s);
        let (addr, pending) = match (kind, lb) {
            (ServiceKind::LoadBalancer, Some(a)) => (a, false),
            (ServiceKind::LoadBalancer, None) => ("no address yet".to_string(), true),
            _ => (service_address(s), false),
        };
        let typ = s
            .spec
            .as_ref()
            .and_then(|s| s.type_.as_deref())
            .unwrap_or("ClusterIP");
        svc_nodes.push(network_node(
            cid,
            now_ms,
            id.clone(),
            name,
            ns,
            &format!("Service ({typ})"),
            kind.as_str(),
            addr,
            pods.len(),
            pending,
        ));
        if let (Some(ip), Some(last)) = (cluster_ip(s), svc_nodes.last_mut()) {
            set_text(last, "ip", ip);
        }
        for pod in pods {
            edges.push(route_edge(&id, pod));
        }
    }
    svc_nodes
}

/// Ingress nodes, each with a route to the (drawn) Services it sends to.
fn build_ingresses(
    cid: &str,
    now_ms: i64,
    ingresses: &BTreeMap<String, &Ingress>,
    drawn_keys: &HashSet<&str>,
    edges: &mut Vec<Edge>,
) -> Vec<PbNode> {
    let mut ing_nodes = Vec::new();
    for (key, ing) in ingresses {
        let (ns, name) = ns_name(&ing.metadata);
        let targets: Vec<&str> = ingress_targets(ing)
            .into_iter()
            .filter(|t| drawn_keys.contains(format!("{ns}/{t}").as_str()))
            .collect();
        let id = ing_id(cid, key);
        ing_nodes.push(network_node(
            cid,
            now_ms,
            id.clone(),
            name,
            ns,
            "Ingress",
            "ingress",
            ingress_hosts(ing),
            targets.len(),
            false,
        ));
        for t in targets {
            edges.push(route_edge(&id, &svc_id(cid, &format!("{ns}/{t}"))));
        }
    }
    ing_nodes
}

/// Gateway API: HTTPRoute nodes (routed to the Services they send to) and Gateway nodes (routed to the routes attached to them).
fn build_gateway_routes(
    cid: &str,
    i: &Inputs<'_>,
    drawn_keys: &HashSet<&str>,
    edges: &mut Vec<Edge>,
) -> (Vec<PbNode>, Vec<PbNode>) {
    let mut route_nodes = Vec::new();
    for obj in i.routes {
        let (ns, name) = ns_name(&obj.metadata);
        let id = format!("{cid}:rt:{ns}:{name}");
        let targets: Vec<String> = route_backends(obj)
            .into_iter()
            .filter(|t| drawn_keys.contains(t.as_str()))
            .collect();
        route_nodes.push(network_node(
            cid,
            i.now_ms,
            id.clone(),
            name,
            ns,
            "HTTPRoute",
            "httproute",
            route_hosts(obj),
            targets.len(),
            false,
        ));
        for t in &targets {
            edges.push(route_edge(&id, &svc_id(cid, t)));
        }
    }

    let mut gw_nodes = Vec::new();
    for gw in i.gateways {
        let (ns, name) = ns_name(&gw.metadata);
        let key = format!("{ns}/{name}");
        let id = gw_id(cid, &key);
        let attached: Vec<String> = i
            .routes
            .iter()
            .filter(|r| route_parents(r).contains(&key))
            .map(|r| {
                let (rns, rname) = ns_name(&r.metadata);
                format!("{cid}:rt:{rns}:{rname}")
            })
            .collect();
        let pending = gw.data["status"]["addresses"]
            .as_array()
            .is_none_or(Vec::is_empty);
        gw_nodes.push(network_node(
            cid,
            i.now_ms,
            id.clone(),
            name,
            ns,
            "Gateway",
            "gateway",
            gateway_address(gw),
            attached.len(),
            pending,
        ));
        for r in &attached {
            edges.push(route_edge(&id, r));
        }
    }
    (gw_nodes, route_nodes)
}

/// The "Outside" node (LAN / internet): a route to every Ingress and Gateway, and to each Service that is reachable on its own
/// (a load balancer or a node port). `None` when there is nothing to reach from it.
fn build_outside(
    cid: &str,
    now_ms: i64,
    ing_nodes: &[PbNode],
    gw_nodes: &[PbNode],
    drawn: &BTreeMap<String, &Service>,
    edges: &mut Vec<Edge>,
) -> Option<PbNode> {
    let outside = format!("{cid}:outside");
    let reachable: Vec<String> = ing_nodes
        .iter()
        .chain(gw_nodes.iter())
        .map(|n| n.id.clone())
        .chain(
            drawn
                .iter()
                .filter(|(_, s)| service_kind(s) != ServiceKind::ClusterIp)
                .map(|(k, _)| svc_id(cid, k)),
        )
        .collect();
    if reachable.is_empty() {
        return None;
    }
    let node = network_node(
        cid,
        now_ms,
        outside.clone(),
        "outside",
        "",
        "Outside",
        "outside",
        "LAN · internet".to_string(),
        reachable.len(),
        false,
    );
    for to in &reachable {
        edges.push(route_edge(&outside, to));
    }
    Some(node)
}

/// NetworkPolicy nodes, each with a route to the (drawn) pods it applies to. A policy that applies to nothing drawn is left out.
fn build_policies(cid: &str, i: &Inputs<'_>, edges: &mut Vec<Edge>) -> Vec<PbNode> {
    let mut policy_nodes = Vec::new();
    for p in i.policies {
        let (ns, name) = ns_name(&p.metadata);
        let Some(selector) = p.spec.as_ref().map(|s| s.pod_selector.clone()) else {
            continue;
        };
        let applies: Vec<String> = i
            .pods
            .iter()
            .filter(|pod| pod.metadata.namespace.as_deref().unwrap_or_default() == ns)
            .filter(|pod| {
                selects(
                    &selector.clone().unwrap_or_default(),
                    &pod.metadata.labels.clone().unwrap_or_default(),
                )
            })
            .map(|pod| {
                format!(
                    "{cid}:p:{ns}:{}",
                    pod.metadata.name.as_deref().unwrap_or_default()
                )
            })
            .filter(|id| i.pod_ids.contains(id))
            .collect();
        if applies.is_empty() {
            continue;
        }
        let id = format!("{cid}:np:{ns}:{name}");
        policy_nodes.push(network_node(
            cid,
            i.now_ms,
            id.clone(),
            name,
            ns,
            "NetworkPolicy",
            "policy",
            policy_summary(p),
            applies.len(),
            false,
        ));
        for pod in &applies {
            edges.push(route_edge(&id, pod));
        }
    }
    policy_nodes
}

pub fn build(i: &Inputs<'_>) -> (Vec<PbNode>, Vec<Edge>) {
    let cid = i.source_id;
    let behind = pods_behind(i);
    let mut services: BTreeMap<String, &Service> = BTreeMap::new();
    for s in i.services {
        let (ns, name) = ns_name(&s.metadata);
        if s.spec.as_ref().and_then(|s| s.type_.as_deref()) != Some("ExternalName") {
            services.insert(format!("{ns}/{name}"), s);
        }
    }
    let mut ingresses: BTreeMap<String, &Ingress> = BTreeMap::new();
    for ing in i.ingresses {
        let (ns, name) = ns_name(&ing.metadata);
        ingresses.insert(format!("{ns}/{name}"), ing);
    }

    // a Service is worth drawing when something is behind it, an Ingress or an HTTPRoute sends to it, or it can be reached from outside
    let targeted: HashSet<String> = ingresses
        .iter()
        .flat_map(|(key, ing)| {
            let ns = key.split('/').next().unwrap_or_default().to_string();
            ingress_targets(ing)
                .into_iter()
                .map(move |s| format!("{ns}/{s}"))
        })
        .chain(i.routes.iter().flat_map(route_backends))
        .collect();
    let drawn: BTreeMap<String, &Service> = services
        .into_iter()
        .filter(|(key, s)| {
            behind.get(key).is_some_and(|p| !p.is_empty())
                || targeted.contains(key)
                || service_kind(s) != ServiceKind::ClusterIp
        })
        .collect();
    // Built once so each section below checks membership in O(1), instead of scanning `drawn` for every ingress/route target.
    let drawn_keys: HashSet<&str> = drawn.keys().map(String::as_str).collect();

    let mut edges = Vec::new();
    let svc_nodes = build_services(cid, i.now_ms, &drawn, &behind, &mut edges);
    let ing_nodes = build_ingresses(cid, i.now_ms, &ingresses, &drawn_keys, &mut edges);
    let (gw_nodes, route_nodes) = build_gateway_routes(cid, i, &drawn_keys, &mut edges);
    let outside_node = build_outside(cid, i.now_ms, &ing_nodes, &gw_nodes, &drawn, &mut edges);
    let policy_nodes = build_policies(cid, i, &mut edges);

    let mut nodes = Vec::new();
    nodes.extend(outside_node);
    nodes.extend(ing_nodes);
    nodes.extend(gw_nodes);
    nodes.extend(route_nodes);
    nodes.extend(svc_nodes);
    nodes.extend(policy_nodes);
    (nodes, edges)
}

#[cfg(test)]
#[path = "../../tests/unit/k8s_network.rs"]
mod tests;
