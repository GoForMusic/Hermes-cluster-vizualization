//! Two real hubs, one the master and one a member: pairing with the code, the master controlling the member through the link, what the member
//! sees showing up in the master, and the way back.

use std::net::SocketAddr;
use std::time::Duration;

use hermes_agentkit::{Config, ISink, Timing, Uplink};
use hermes_hub::app::{Hub, Settings};
use hermes_hub::database::Repositories;
use hermes_proto::v1::{Node, NodeKind, Own, Provider};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

struct Rig {
    base: String,
    http: Client,
    _web: tempfile::TempDir,
}

impl Rig {
    async fn start() -> Self {
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("index.html"), "<h1>hermes</h1>").unwrap();
        let hub = Hub::new(
            Repositories::sqlite_in_memory().unwrap(),
            Settings {
                web: web.path().into(),
                agent_image: "registry.example.com/agent:1.2.3".into(),
                agent_image_windows: String::new(),
                agent_pull_secret: String::new(),
                tls_enabled: false,
                federation_ca: None,
            },
        );
        hub.spawn_background();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(
            axum::serve(
                listener,
                hub.router()
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .into_future(),
        );
        Self {
            base: format!("http://{addr}"),
            http: Client::builder()
                .cookie_store(true)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            _web: web,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn get(&self, path: &str) -> Response {
        self.http.get(self.url(path)).send().await.unwrap()
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> Response {
        self.http
            .request(method, self.url(path))
            .header("X-Requested-With", "hermes")
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn post(&self, path: &str, body: Value) -> Response {
        self.send(reqwest::Method::POST, path, body).await
    }

    async fn setup_admin(&self) {
        let r = self
            .post(
                "/api/auth/setup",
                json!({"username": "admin", "password": PASSWORD}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
    }
}

async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..120 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

/// A master and a member that was never set up, waiting with a pairing code.
async fn waiting_member() -> (Rig, String) {
    let member = Rig::start().await;
    let r = member
        .post(
            "/api/federation/member",
            json!({"agentAddress": "http://agents.dc2.example:8765"}),
        )
        .await;
    assert_eq!(
        r.status(),
        StatusCode::OK,
        "a fresh hub can be set up to be managed"
    );
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["member"]["state"], "waiting");
    let code = body["code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 14, "{code}"); // XXXX-XXXX-XXXX
    (member, code)
}

async fn pair(master: &Rig, member: &Rig, code: &str) -> String {
    let r = master
        .post(
            "/api/federation/datacenters",
            json!({"name": "dc2", "address": member.base, "code": code}),
        )
        .await;
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "{}",
        r.text().await.unwrap()
    );
    let view: Value = master.get("/api/federation").await.json().await.unwrap();
    view["datacenters"][0]["id"].as_str().unwrap().to_string()
}

fn cluster(id: &str) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::Cluster.into(),
        name: "lab".into(),
        provider: Provider::Kubernetes.into(),
        own: Own::Ok.into(),
        ..Default::default()
    }
}

fn agent(member: &Rig, source: &str, token: &str) -> Uplink {
    let env = [
        ("HUB_URL", member.base.as_str()),
        ("SOURCE_ID", source),
        ("SOURCE_NAME", "lab"),
        ("TOKEN", token),
        ("AGENT_ID", "pod-1"),
        ("AGENT_HOST", ""),
    ];
    let cfg = Config::from_lookup("node", "1.0.0", |k| {
        env.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| (*v).to_string())
    })
    .unwrap();
    Uplink::new(&cfg).unwrap().with_timing(Timing {
        flush: Duration::from_millis(30),
        backoff_min: Duration::from_millis(30),
        backoff_max: Duration::from_millis(100),
    })
}

#[tokio::test]
async fn a_member_is_controlled_from_the_master_and_what_it_sees_shows_up_there() {
    let (member, code) = waiting_member().await;
    let master = Rig::start().await;
    master.setup_admin().await;
    let dc = pair(&master, &member, &code).await;

    eventually("the datacenter is connected", || async {
        let v: Value = master.get("/api/federation").await.json().await.unwrap();
        v["datacenters"][0]["state"] == "connected"
    })
    .await;
    // the master records what comes over the link, next to what its own agents say
    let r = master
        .send(
            reqwest::Method::PUT,
            "/api/comm-log",
            json!({"enabled": true}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = master.get("/api/federation").await.json().await.unwrap();
    assert_eq!(
        v["datacenters"][0]["agentAddress"], "http://agents.dc2.example:8765",
        "the master learns where the agents of the datacenter find it"
    );

    // the member is locked: no logins, no setup, nothing for a browser
    let status: Value = member.get("/api/auth/status").await.json().await.unwrap();
    assert_eq!(status["member"]["state"], "paired");
    assert_eq!(
        status["member"]["managedBy"], "dc1",
        "it says which hub manages it, not what that hub calls it"
    );
    assert_eq!(status["member"]["datacenter"], "dc2");
    assert_eq!(status["setupRequired"], false);
    let r = member
        .post(
            "/api/auth/setup",
            json!({"username": "admin", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        member.get("/api/sources").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        member.get("/api/snapshot").await.status(),
        StatusCode::UNAUTHORIZED
    );

    // a source is added on the member, from the master; its manifest points the agent at the member
    let r = master
        .post(
            &format!("/api/dc/{dc}/sources"),
            json!({"name": "dc2-swarm", "type": "Docker Swarm (agent)", "hubUrl": "http://agents.dc2.example:8765"}),
        )
        .await;
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "{}",
        r.text().await.unwrap()
    );
    let body: Value = r.json().await.unwrap();
    let id = body["id"].as_str().unwrap().to_string();
    assert!(
        body["install"]
            .as_str()
            .unwrap()
            .contains("http://agents.dc2.example:8765")
    );
    let token = body["install"]
        .as_str()
        .unwrap()
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("TOKEN: \"")
                .or_else(|| l.trim().strip_prefix("token: \""))
        })
        .unwrap()
        .trim_end_matches('"')
        .to_string();

    // the agent reports to the member only; the master shows it
    let a = agent(&member, &id, &token);
    a.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(a.clone().run());
    eventually("the node reaches the master", || async {
        let snap: Value = master.get("/api/snapshot").await.json().await.unwrap();
        snap["nodes"]
            .as_array()
            .is_some_and(|n| n.iter().any(|n| n["id"] == id.as_str()))
    })
    .await;
    eventually(
        "the source is listed in the master with its datacenter",
        || async {
            let list: Value = master.get("/api/sources").await.json().await.unwrap();
            list.as_array().unwrap().iter().any(|s| {
                s["id"] == id.as_str()
                    && s["dc"] == dc.as_str()
                    && s["dcName"] == "dc2"
                    && s["state"] == "connected"
            })
        },
    )
    .await;

    // a change the agent makes after that follows
    a.status(&id, Own::Crit, "unreachable");
    eventually("the status follows", || async {
        let snap: Value = master.get("/api/snapshot").await.json().await.unwrap();
        snap["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == id.as_str() && n["own"] == "crit")
    })
    .await;

    // the tunnel carries the allowed calls only
    let r = master
        .send(
            reqwest::Method::PATCH,
            &format!("/api/dc/{dc}/sources/{id}"),
            json!({"name": "renamed"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    eventually("the new name is listed", || async {
        let list: Value = master.get("/api/sources").await.json().await.unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id.as_str() && s["name"] == "renamed")
    })
    .await;
    for forbidden in ["auth/password", "settings", "comm-log"] {
        let r = master
            .post(&format!("/api/dc/{dc}/{forbidden}"), json!({}))
            .await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{forbidden}");
    }
    let r = master.post("/api/dc/nope/sources", json!({})).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    // removing the source on the member takes its nodes out of the master
    let r = master
        .send(
            reqwest::Method::DELETE,
            &format!("/api/dc/{dc}/sources/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    eventually("the nodes are gone", || async {
        let snap: Value = master.get("/api/snapshot").await.json().await.unwrap();
        snap["nodes"].as_array().unwrap().is_empty()
    })
    .await;

    // agent -> child hub -> this hub: the second half is in this hub's log, without the content of what was asked
    let log: Value = master.get("/api/comm-log").await.json().await.unwrap();
    let rows = log["entries"].as_array().unwrap();
    let has = |kind: &str, text: &str| {
        rows.iter().any(|r| {
            r["kind"] == kind
                && r["agent"] == "hub dc2"
                && r["summary"].as_str().unwrap().contains(text)
        })
    };
    assert!(has("topology", "source"), "{rows:?}");
    assert!(has("request", "POST /api/sources"), "{rows:?}");
    assert!(has("response", "answer 201"), "{rows:?}");
    assert!(!log.to_string().contains(&token), "no token in the log");

    // taking the datacenter away lets the member go back to being on its own
    let r = master
        .send(
            reqwest::Method::DELETE,
            &format!("/api/federation/datacenters/{dc}"),
            json!({}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.json::<Value>().await.unwrap()["released"], true);
    eventually("the member has logins again", || async {
        let status: Value = member.get("/api/auth/status").await.json().await.unwrap();
        status["setupRequired"] == true && status.get("member").is_none()
    })
    .await;
}

#[tokio::test]
async fn a_wrong_or_used_code_pairs_nothing() {
    let (member, code) = waiting_member().await;
    let master = Rig::start().await;
    master.setup_admin().await;

    let r = master
        .post(
            "/api/federation/datacenters",
            json!({"name": "dc2", "address": member.base, "code": "AAAA-BBBB-CCCC"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
    let view: Value = master.get("/api/federation").await.json().await.unwrap();
    assert!(view["datacenters"].as_array().unwrap().is_empty());
    let status: Value = member.get("/api/auth/status").await.json().await.unwrap();
    assert_eq!(
        status["member"]["state"], "waiting",
        "a wrong code leaves the member waiting"
    );

    pair(&master, &member, &code).await;
    // the code is used up: another master cannot take the member
    let other = Rig::start().await;
    other.setup_admin().await;
    let r = other
        .post(
            "/api/federation/datacenters",
            json!({"name": "dc2", "address": member.base, "code": code}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_hub_that_is_not_waiting_cannot_be_paired_and_a_forged_link_header_opens_nothing() {
    let master = Rig::start().await;
    master.setup_admin().await;
    let plain = Rig::start().await; // a hub nobody asked to be managed
    let r = master
        .post(
            "/api/federation/datacenters",
            json!({"name": "dc2", "address": plain.base, "code": "AAAA-BBBB-CCCC"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::BAD_GATEWAY);

    let r = plain
        .http
        .get(plain.url("/api/sources"))
        .header("x-hermes-link", "guess")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

    // the member side needs a login (or a hub with none yet) to be turned on or off
    plain.setup_admin().await;
    let anonymous = Client::new();
    let r = anonymous
        .post(plain.url("/api/federation/member"))
        .header("X-Requested-With", "hermes")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn while_the_link_is_down_the_sources_of_the_datacenter_are_unreachable_not_ok() {
    let (member, code) = waiting_member().await;
    let master = Rig::start().await;
    master.setup_admin().await;
    let dc = pair(&master, &member, &code).await;
    eventually("connected", || async {
        let v: Value = master.get("/api/federation").await.json().await.unwrap();
        v["datacenters"][0]["state"] == "connected"
    })
    .await;
    let r = master
        .post(
            &format!("/api/dc/{dc}/sources"),
            json!({"name": "dc2-swarm", "type": "Docker Swarm (agent)", "hubUrl": "http://x:1"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let id = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    eventually("the source is listed", || async {
        let list: Value = master.get("/api/sources").await.json().await.unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id.as_str())
    })
    .await;

    drop(member); // the listener task stays, but nothing answers for the member's hub any more: stop it hard
    // (the Rig keeps the server alive for the test's life, so simulate the cut by pointing the datacenter at nothing)
    let r = master
        .send(
            reqwest::Method::PATCH,
            &format!("/api/federation/datacenters/{dc}"),
            json!({"address": "http://127.0.0.1:9"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    eventually("the datacenter is unreachable", || async {
        let v: Value = master.get("/api/federation").await.json().await.unwrap();
        v["datacenters"][0]["state"] == "unreachable"
    })
    .await;
    let list: Value = master.get("/api/sources").await.json().await.unwrap();
    let source = list
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .expect("the source stays listed");
    assert_eq!(source["state"], "error");
    // and a request to it fails clearly instead of hanging
    let r = master
        .post(&format!("/api/dc/{dc}/sources"), json!({}))
        .await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn two_hubs_that_already_run_are_joined_and_the_one_that_becomes_a_member_keeps_its_sources()
{
    // the child hub has been in use for a while: an admin, a source, a registry
    let child = Rig::start().await;
    child.setup_admin().await;
    let r = child
        .post(
            "/api/sources",
            json!({"name": "old-swarm", "type": "Docker Swarm (agent)", "hubUrl": child.base}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let source = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // the parent too, with a source of its own that has the same name
    let master = Rig::start().await;
    master.setup_admin().await;
    let r = master
        .post(
            "/api/sources",
            json!({"name": "old-swarm", "type": "Docker Swarm (agent)", "hubUrl": master.base}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);

    // from Settings of the child, as its admin (not at first run)
    let r = child
        .post(
            "/api/federation/member",
            json!({"agentAddress": "http://agents.dc2.example:8765"}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let code = r.json::<Value>().await.unwrap()["code"]
        .as_str()
        .unwrap()
        .to_string();
    // until a master comes, the admin still works
    assert_eq!(child.get("/api/sources").await.status(), StatusCode::OK);

    let dc = pair(&master, &child, &code).await;
    eventually("the child is connected", || async {
        let v: Value = master.get("/api/federation").await.json().await.unwrap();
        v["datacenters"][0]["state"] == "connected"
    })
    .await;

    // the child's sources are the master's to see; same names in two hubs do not clash
    eventually("both sources are listed in the master", || async {
        let list: Value = master.get("/api/sources").await.json().await.unwrap();
        let list = list.as_array().unwrap();
        list.len() == 2
            && list
                .iter()
                .any(|s| s["id"] == source.as_str() && s["dc"] == dc.as_str())
            && list.iter().any(|s| s["dc"].is_null())
    })
    .await;

    // and the child's own admin is locked out: that session no longer opens anything
    assert_eq!(
        child.get("/api/sources").await.status(),
        StatusCode::UNAUTHORIZED
    );
    let r = child
        .post(
            "/api/auth/login",
            json!({"username": "admin", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}
