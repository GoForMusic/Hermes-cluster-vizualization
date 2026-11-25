//! The hub as the outside world sees it: a real listener, a browser-like HTTP client with cookies, an event stream that is really read,
//! and a real agent (`Uplink`) talking gRPC to it on the same port.

use std::net::SocketAddr;
use std::time::Duration;

use std::sync::{Arc, Mutex};

use hermes_agentkit::{Config, ISink, IUpgrader, Timing, UpgradeOutcome, Uplink};
use hermes_hub::app::{Hub, Settings};
use hermes_hub::database::Repositories;
use hermes_proto::v1::{Node, NodeKind, Own, Provider};
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

struct Rig {
    base: String,
    addr: SocketAddr,
    http: Client,
    _web: tempfile::TempDir,
}

impl Rig {
    async fn start() -> Self {
        Self::start_tls(false).await
    }

    /// `tls_enabled` only changes whether the session cookie gets `Secure` — this rig still listens in plain HTTP either way
    /// (a real TLS listener is covered by `hub_tls.rs`), it is just telling the hub it is behind one.
    async fn start_tls(tls_enabled: bool) -> Self {
        Self::start_with(tls_enabled, "").await
    }

    /// A hub mounted under `base_path` (`HUB_BASE_PATH`): `rig.url(..)` already includes it, `rig.addr` does not.
    async fn start_at(base_path: &str) -> Self {
        Self::start_with(false, base_path).await
    }

    async fn start_with(tls_enabled: bool, base_path: &str) -> Self {
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("index.html"), "<h1>infraviz</h1>").unwrap();
        std::fs::write(web.path().join("app.js"), "export {}").unwrap();
        std::fs::write(web.path().join("font.woff2"), "wOF2").unwrap();
        let hub = Hub::new(
            Repositories::sqlite_in_memory().unwrap(),
            Settings {
                web: web.path().into(),
                base_path: base_path.into(),
                public_url: None,
                agent_image: "registry.example.com/agent:1.2.3".into(),
                agent_image_windows: String::new(),
                agent_pull_secret: String::new(),
                tls_enabled,
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
        let http = Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        Self {
            base: format!("http://{addr}{base_path}"),
            addr,
            http,
            _web: web,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn get(&self, path: &str) -> Response {
        self.http.get(self.url(path)).send().await.unwrap()
    }

    /// A call that changes something, the way the web app makes it.
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

    async fn setup_admin(&self, public_view: bool) {
        let r = self
            .post(
                "/api/auth/setup",
                json!({"username": "admin", "password": PASSWORD, "publicView": public_view}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
    }

    /// Adds an agent source the way the admin page does; returns its id and token.
    async fn add_source(&self, kind: &str) -> (String, String) {
        self.add_source_named(kind, "lab").await
    }

    async fn add_source_named(&self, kind: &str, name: &str) -> (String, String) {
        let r = self
            .post(
                "/api/sources",
                json!({"name": name, "type": kind, "hubUrl": self.base}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
        let body: Value = r.json().await.unwrap();
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
        (body["id"].as_str().unwrap().to_string(), token)
    }

    fn agent(&self, source: &str, token: &str) -> Uplink {
        self.agent_version(source, token, "1.0.0")
    }

    fn agent_version(&self, source: &str, token: &str, version: &str) -> Uplink {
        let env = [
            ("HUB_URL", self.base.as_str()),
            ("SOURCE_ID", source),
            ("SOURCE_NAME", "lab"),
            ("TOKEN", token),
            ("AGENT_ID", "pod-1"),
            ("AGENT_HOST", ""),
        ];
        let cfg = Config::from_lookup("node", version, |k| {
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

/// Polls until `check` holds, or fails after a few seconds.
async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..100 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

#[tokio::test]
async fn serves_the_web_app_and_never_lets_it_be_cached() {
    let rig = Rig::start().await;
    let index = rig.get("/").await;
    assert_eq!(
        (
            index.status(),
            index.headers()["cache-control"].to_str().unwrap()
        ),
        (StatusCode::OK, "no-store")
    );
    // the page is the build's `index.html` with the (here empty) base path put in
    assert_eq!(
        index.text().await.unwrap(),
        "<base href=\"/\"><script>window.__HUB_BASE__=\"\"</script><h1>infraviz</h1>"
    );
    let js = rig.get("/app.js").await;
    assert!(
        js.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("javascript"),
        "{:?}",
        js.headers()
    );
    assert_eq!(
        rig.get("/font.woff2").await.headers()["content-type"],
        "font/woff2"
    );
    assert_eq!(rig.get("/nope.txt").await.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        rig.get("/api/health").await.json::<Value>().await.unwrap(),
        json!({"ok": true, "version": hermes_hub::version::VERSION})
    );
}

#[tokio::test]
async fn under_a_base_path_the_hub_answers_only_there() {
    let rig = Rig::start_at("/hermes").await;
    for page in ["/", "/index.html"] {
        let html = rig.get(page).await.text().await.unwrap();
        assert!(
            html.contains(
                "<base href=\"/hermes/\"><script>window.__HUB_BASE__=\"/hermes\"</script>"
            ),
            "{page}: {html}"
        );
    }
    assert_eq!(rig.get("/app.js").await.status(), StatusCode::OK);
    assert_eq!(
        rig.get("/api/health").await.status(),
        StatusCode::OK,
        "the API is under the prefix too"
    );
    // without the prefix there is nothing: the proxy only sends /hermes, and anything else is not this hub's
    let outside = |path: &str| format!("http://{}{path}", rig.addr);
    for path in ["/", "/app.js", "/api/health"] {
        let r = rig.http.get(outside(path)).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
    }
    // `/hermes` without the slash is the same page (a <base> with an absolute href does not care)
    let bare = rig.http.get(outside("/hermes")).send().await.unwrap();
    assert_eq!(bare.status(), StatusCode::OK);
}

#[tokio::test]
async fn under_a_base_path_the_session_cookie_belongs_to_that_path() {
    let rig = Rig::start_at("/hermes").await;
    let r = rig
        .post(
            "/api/auth/setup",
            json!({"username": "admin", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("; Path=/hermes;"),
        "{:?}",
        r.headers()
    );
    // the cookie works for the API under the prefix, and logging out clears it on the same path
    assert_eq!(rig.get("/api/sources").await.status(), StatusCode::OK);
    let out = rig.post("/api/auth/logout", json!({})).await;
    assert!(
        out.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Path=/hermes;")
    );
}

#[tokio::test]
async fn an_agent_reaches_a_hub_under_a_base_path_with_the_path_in_its_hub_url() {
    let rig = Rig::start_at("/hermes").await;
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;
    let agent = rig.agent(&id, &token); // HUB_URL = http://127.0.0.1:port/hermes
    agent.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(agent.clone().run());
    eventually("the source is connected", || async {
        rig.get("/api/sources").await.json::<Value>().await.unwrap()[0]["state"] == "connected"
    })
    .await;
}

#[tokio::test]
async fn the_admin_manages_accounts_and_groups_and_a_disabled_account_cannot_log_in() {
    let rig = Rig::start().await;
    // nobody is logged in: nothing about accounts is shown
    assert_eq!(
        rig.get("/api/users").await.status(),
        StatusCode::UNAUTHORIZED
    );
    rig.setup_admin(false).await;

    // the account the setup made is the break-glass one
    let users: Value = rig.get("/api/users").await.json().await.unwrap();
    assert_eq!(users[0]["username"], "admin");
    assert_eq!(users[0]["breakGlass"], true);
    assert!(!users.to_string().contains("argon2"), "{users}");

    let made = rig
        .post(
            "/api/users",
            json!({"username": "alice", "password": PASSWORD, "email": "alice@example.com"}),
        )
        .await;
    assert_eq!(made.status(), StatusCode::CREATED);
    let alice: Value = made.json().await.unwrap();
    let id = alice["id"].as_i64().unwrap();
    assert_eq!(
        rig.post(
            "/api/users",
            json!({"username": "alice", "password": PASSWORD})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        rig.post(
            "/api/users",
            json!({"username": "bobby", "password": "short"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    // alice logs in with her own client: a second browser
    let alice_http = Client::builder().cookie_store(true).build().unwrap();
    let login = |http: &Client, pw: &str| {
        http.post(rig.url("/api/auth/login"))
            .header("X-Requested-With", "hermes")
            .json(&json!({"username": "alice", "password": pw}))
            .send()
    };
    assert_eq!(
        login(&alice_http, PASSWORD).await.unwrap().status(),
        StatusCode::OK
    );
    let me = || alice_http.get(rig.url("/api/auth/status")).send();
    assert_eq!(
        me().await.unwrap().json::<Value>().await.unwrap()["authenticated"],
        true
    );

    // disabling ends her session at once and keeps her out
    let off = rig
        .send(
            reqwest::Method::PATCH,
            &format!("/api/users/{id}"),
            json!({"disabled": true}),
        )
        .await;
    assert_eq!(off.status(), StatusCode::OK);
    assert_eq!(
        me().await.unwrap().json::<Value>().await.unwrap()["authenticated"],
        false
    );
    assert_eq!(
        login(&alice_http, PASSWORD).await.unwrap().status(),
        StatusCode::UNAUTHORIZED,
        "told the same as a wrong password"
    );
    rig.send(
        reqwest::Method::PATCH,
        &format!("/api/users/{id}"),
        json!({"disabled": false}),
    )
    .await;
    assert_eq!(
        login(&alice_http, PASSWORD).await.unwrap().status(),
        StatusCode::OK
    );

    // a password the admin sets works, the old one does not
    rig.send(
        reqwest::Method::PATCH,
        &format!("/api/users/{id}"),
        json!({"password": "a brand new password"}),
    )
    .await;
    assert_eq!(
        login(&alice_http, PASSWORD).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&alice_http, "a brand new password")
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    // groups
    let g = rig
        .post(
            "/api/groups",
            json!({"name": "ops", "description": "on call"}),
        )
        .await;
    assert_eq!(g.status(), StatusCode::CREATED);
    let gid = g.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
    let members = rig
        .send(
            reqwest::Method::PUT,
            &format!("/api/groups/{gid}/members"),
            json!({"users": [id]}),
        )
        .await;
    assert_eq!(
        members.json::<Value>().await.unwrap()["members"],
        json!([id])
    );
    let users: Value = rig.get("/api/users").await.json().await.unwrap();
    assert_eq!(users[1]["groups"], json!([gid]));
    assert_eq!(
        rig.post("/api/groups", json!({"name": "ops"}))
            .await
            .status(),
        StatusCode::CONFLICT
    );

    // the rules: not your own account, never the break-glass one
    let admin_id = users[0]["id"].as_i64().unwrap();
    for body in [json!({"disabled": true}), json!({"password": PASSWORD})] {
        let r = rig
            .send(
                reqwest::Method::PATCH,
                &format!("/api/users/{admin_id}"),
                body,
            )
            .await;
        assert_eq!(r.status(), StatusCode::CONFLICT);
    }
    let del = rig
        .send(
            reqwest::Method::DELETE,
            &format!("/api/users/{admin_id}"),
            json!({}),
        )
        .await;
    assert_eq!(del.status(), StatusCode::CONFLICT);
    assert_eq!(
        rig.send(reqwest::Method::DELETE, "/api/users/9999", json!({}))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    // deleting her takes her out of the group; her session is gone
    let del = rig
        .send(
            reqwest::Method::DELETE,
            &format!("/api/users/{id}"),
            json!({}),
        )
        .await;
    assert_eq!(del.status(), StatusCode::OK);
    let groups: Value = rig.get("/api/groups").await.json().await.unwrap();
    let ops = groups
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "ops")
        .unwrap();
    assert_eq!(ops["members"], json!([]));
    assert_eq!(
        me().await.unwrap().json::<Value>().await.unwrap()["authenticated"],
        false
    );
}

/// A second browser, logged in as `name` (made by the admin of `rig`, with `PASSWORD`).
async fn user_client(rig: &Rig, name: &str) -> (Client, i64) {
    let made = rig
        .post(
            "/api/users",
            json!({"username": name, "password": PASSWORD}),
        )
        .await;
    assert_eq!(made.status(), StatusCode::CREATED);
    let id = made.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
    let http = Client::builder().cookie_store(true).build().unwrap();
    let r = http
        .post(rig.url("/api/auth/login"))
        .header("X-Requested-With", "hermes")
        .json(&json!({"username": name, "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    (http, id)
}

async fn status_of(rig: &Rig, http: &Client, method: reqwest::Method, path: &str) -> StatusCode {
    http.request(method, rig.url(path))
        .header("X-Requested-With", "hermes")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .status()
}

fn id_named(list: &Value, name: &str) -> i64 {
    list.as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == name)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn a_hub_starts_with_the_policies_and_groups_an_update_must_find() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let policies: Value = rig.get("/api/policies").await.json().await.unwrap();
    let names: Vec<&str> = policies
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["admin", "read-only"]);
    assert!(policies[0]["builtin"].as_bool().unwrap());
    let groups: Value = rig.get("/api/groups").await.json().await.unwrap();
    let by = |n: &str| {
        groups
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == n)
            .unwrap()
            .clone()
    };
    let (owners, viewers, everyone) = (by("owners"), by("viewers"), by("everyone"));
    assert_eq!(owners["policies"], json!([policies[0]["id"]]));
    assert_eq!(viewers["policies"], json!([policies[1]["id"]]));
    assert_eq!(
        everyone["policies"],
        json!([]),
        "everyone has no policy until you give it one"
    );
    assert_eq!(everyone["system"], true);
    // the account of the first-run setup is an owner
    let admin_id = rig.get("/api/users").await.json::<Value>().await.unwrap()[0]["id"].clone();
    assert_eq!(owners["members"], json!([admin_id]));
    assert_eq!(
        everyone["members"],
        json!([admin_id]),
        "everyone lists every user"
    );
}

#[tokio::test]
async fn what_a_user_may_do_comes_from_the_policies_of_their_groups() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (bob, bob_id) = user_client(&rig, "bobby").await;
    let get = reqwest::Method::GET;
    let post = reqwest::Method::POST;

    // in no group: nothing, and the page that says who they are still works
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/sources").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/snapshot").await,
        StatusCode::FORBIDDEN
    );
    let me: Value = bob
        .get(rig.url("/api/auth/status"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        (me["authenticated"].clone(), me["grants"].clone()),
        (json!(true), json!([]))
    );
    let refused: Value = bob
        .get(rig.url("/api/sources"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(refused["error"], "not allowed: read on sources");

    // viewers: read-only
    let groups: Value = rig.get("/api/groups").await.json().await.unwrap();
    let viewers = id_named(&groups, "viewers");
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{viewers}/members"),
        json!({"users": [bob_id]}),
    )
    .await;
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/sources").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/users").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&rig, &bob, post.clone(), "/api/sources").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(&rig, &bob, reqwest::Method::DELETE, "/api/sources/s1").await,
        StatusCode::FORBIDDEN
    );
    // their own password is theirs whatever the policies say
    let pw = bob
        .post(rig.url("/api/auth/password"))
        .header("X-Requested-With", "hermes")
        .json(&json!({"current": PASSWORD, "next": "a different long password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(pw.status(), StatusCode::OK);

    // a policy of the admin's own: write on sources, but never on the prod ones
    let made = rig
        .post(
            "/api/policies",
            json!({"name": "source-editor", "hcl": "path \"sources/*\" {\n capabilities = [\"read\", \"update\"]\n}\npath \"sources/prod-*\" {\n capabilities = [\"deny\"]\n}\n"}),
        )
        .await;
    assert_eq!(made.status(), StatusCode::CREATED);
    let editor = made.json::<Value>().await.unwrap()["id"].as_i64().unwrap();
    let devs = rig
        .post("/api/groups", json!({"name": "dev"}))
        .await
        .json::<Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{devs}/policies"),
        json!({"policies": [editor]}),
    )
    .await;
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{devs}/members"),
        json!({"users": [bob_id]}),
    )
    .await;
    let patch = reqwest::Method::PATCH;
    assert_ne!(
        status_of(&rig, &bob, patch.clone(), "/api/sources/s1").await,
        StatusCode::FORBIDDEN,
        "allowed: it got as far as the source (and was refused there, for what it sent)"
    );
    assert_eq!(
        status_of(&rig, &bob, patch, "/api/sources/prod-eu").await,
        StatusCode::FORBIDDEN
    );

    // everyone: give a policy to all of them at once
    let everyone = id_named(
        &rig.get("/api/groups").await.json::<Value>().await.unwrap(),
        "everyone",
    );
    let read_only = id_named(
        &rig.get("/api/policies")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        "read-only",
    );
    // out of viewers, only `dev` is left, and it covers sources and nothing else
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{viewers}/members"),
        json!({"users": []}),
    )
    .await;
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/settings").await,
        StatusCode::FORBIDDEN
    );
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{everyone}/policies"),
        json!({"policies": [read_only]}),
    )
    .await;
    assert_eq!(
        status_of(&rig, &bob, get.clone(), "/api/settings").await,
        StatusCode::OK
    );

    // the break-glass account is not asked at all
    assert_eq!(rig.get("/api/users").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn policies_are_checked_when_they_are_saved_and_the_built_in_ones_stay() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let bad = rig
        .post(
            "/api/policies",
            json!({"name": "oops", "hcl": "path \"a\" {"}),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert!(
        bad.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("line 1")
    );
    let nope = rig
        .post(
            "/api/policies",
            json!({"name": "oops", "hcl": "path \"a\" { capabilities = [\"write\"] }"}),
        )
        .await;
    assert!(
        nope.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("unknown capability")
    );
    assert_eq!(
        rig.post("/api/policies", json!({"name": "Not Ok", "hcl": ""}))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    let ok = rig
        .post("/api/policies", json!({"name": "empty", "hcl": ""}))
        .await;
    assert_eq!(ok.status(), StatusCode::CREATED);
    assert_eq!(
        rig.post("/api/policies", json!({"name": "empty", "hcl": ""}))
            .await
            .status(),
        StatusCode::CONFLICT
    );

    let policies: Value = rig.get("/api/policies").await.json().await.unwrap();
    let admin = id_named(&policies, "admin");
    let gone = rig
        .send(
            reqwest::Method::DELETE,
            &format!("/api/policies/{admin}"),
            json!({}),
        )
        .await;
    assert_eq!(gone.status(), StatusCode::CONFLICT);
    let renamed = rig
        .send(
            reqwest::Method::PATCH,
            &format!("/api/policies/{admin}"),
            json!({"name": "root"}),
        )
        .await;
    assert_eq!(renamed.status(), StatusCode::CONFLICT);
    let edited = rig
        .send(
            reqwest::Method::PATCH,
            &format!("/api/policies/{admin}"),
            json!({"description": "all"}),
        )
        .await;
    assert_eq!(edited.status(), StatusCode::OK);
    let groups: Value = rig.get("/api/groups").await.json().await.unwrap();
    let everyone = id_named(&groups, "everyone");
    assert_eq!(
        rig.send(
            reqwest::Method::DELETE,
            &format!("/api/groups/{everyone}"),
            json!({})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        rig.send(
            reqwest::Method::PUT,
            &format!("/api/groups/{everyone}/members"),
            json!({"users": []})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let empty = id_named(&policies_after(&rig).await, "empty");
    assert_eq!(
        rig.send(
            reqwest::Method::DELETE,
            &format!("/api/policies/{empty}"),
            json!({})
        )
        .await
        .status(),
        StatusCode::OK
    );
}

async fn ask(rig: &Rig, user: i64, cap: &str, path: &str) -> Value {
    rig.get(&format!(
        "/api/policies/check?user={user}&capability={cap}&path={path}"
    ))
    .await
    .json()
    .await
    .unwrap()
}

async fn policies_after(rig: &Rig) -> Value {
    rig.get("/api/policies").await.json().await.unwrap()
}

#[tokio::test]
async fn the_admin_can_ask_what_a_user_would_be_allowed() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (_bob, bob_id) = user_client(&rig, "bobby").await;
    let none = ask(&rig, bob_id, "read", "sources").await;
    assert_eq!(
        (none["allowed"].clone(), none["reason"].as_str().unwrap()),
        (json!(false), "no policy of this user covers it")
    );
    let groups: Value = rig.get("/api/groups").await.json().await.unwrap();
    let viewers = id_named(&groups, "viewers");
    rig.send(
        reqwest::Method::PUT,
        &format!("/api/groups/{viewers}/members"),
        json!({"users": [bob_id]}),
    )
    .await;
    let read = ask(&rig, bob_id, "read", "sources/s1").await;
    assert_eq!(
        (read["allowed"].clone(), read["reason"].as_str().unwrap()),
        (json!(true), "allowed by read-only")
    );
    assert_eq!(
        ask(&rig, bob_id, "delete", "sources/s1").await["allowed"],
        false
    );
    assert_eq!(
        rig.get(&format!(
            "/api/policies/check?user={bob_id}&capability=bogus&path=x"
        ))
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        rig.get("/api/policies/check?user=9999&capability=read&path=x")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_admin_sets_how_long_a_login_lasts_and_it_shows_in_the_cookie() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let now: Value = rig.get("/api/security").await.json().await.unwrap();
    assert_eq!(now, json!({"maxHours": 336, "idleMinutes": 0}));
    let put = |body: Value| rig.send(reqwest::Method::PUT, "/api/security", body);
    assert_eq!(
        put(json!({"maxHours": 0, "idleMinutes": 0})).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put(json!({"maxHours": 8, "idleMinutes": 3})).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        put(json!({"maxHours": 8})).await.status(),
        StatusCode::BAD_REQUEST,
        "both are asked for"
    );
    assert_eq!(
        put(json!({"maxHours": 8, "idleMinutes": 30}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        rig.get("/api/security")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!({"maxHours": 8, "idleMinutes": 30})
    );
    let login = rig
        .http
        .post(rig.url("/api/auth/login"))
        .header("X-Requested-With", "hermes")
        .json(&json!({"username": "admin", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    let cookie = login.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cookie.contains("Max-Age=28800"), "{cookie}");
}

#[tokio::test]
async fn the_first_visit_creates_the_admin_and_logs_them_in() {
    let rig = Rig::start().await;
    let status: Value = rig.get("/api/auth/status").await.json().await.unwrap();
    assert_eq!(
        status,
        json!({"setupRequired": true, "authenticated": false, "publicView": false})
    );

    let no_csrf = rig
        .http
        .post(rig.url("/api/auth/setup"))
        .json(&json!({"username": "admin", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        no_csrf.status(),
        StatusCode::FORBIDDEN,
        "a page from another origin cannot do this"
    );

    let r = rig
        .post(
            "/api/auth/setup",
            json!({"username": "admin", "password": PASSWORD}),
        )
        .await;
    let cookie = r.headers()["set-cookie"].to_str().unwrap().to_string();
    assert_eq!(r.status(), StatusCode::CREATED);
    assert!(
        cookie.starts_with("hermes_session=")
            && cookie.contains("HttpOnly")
            && cookie.contains("SameSite=Strict")
            && !cookie.contains("Secure"),
        "{cookie}"
    );

    let status: Value = rig.get("/api/auth/status").await.json().await.unwrap();
    // the first account is break-glass: it is told it may do everything
    assert_eq!(status["grants"][0]["path"], "*");
    assert_eq!(
        status["grants"][0]["capabilities"],
        json!(["read", "list", "create", "update", "delete"])
    );
    assert_eq!(
        (status["authenticated"].clone(), status["username"].clone()),
        (json!(true), json!("admin"))
    );
    assert_eq!(
        rig.post(
            "/api/auth/setup",
            json!({"username": "x1x", "password": PASSWORD})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn the_cookie_is_secure_behind_an_https_proxy() {
    let rig = Rig::start().await;
    let r = rig
        .http
        .post(rig.url("/api/auth/setup"))
        .header("X-Requested-With", "hermes")
        .header("X-Forwarded-Proto", "https")
        .json(&json!({"username": "admin", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("; Secure")
    );
}

#[tokio::test]
async fn the_cookie_is_secure_when_the_hub_terminates_tls_itself() {
    let rig = Rig::start_tls(true).await;
    let r = rig
        .http
        .post(rig.url("/api/auth/setup"))
        .header("X-Requested-With", "hermes")
        // no X-Forwarded-Proto: nothing is proxying this hub, it is doing TLS itself.
        .json(&json!({"username": "admin", "password": PASSWORD}))
        .send()
        .await
        .unwrap();
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("; Secure")
    );
}

#[tokio::test]
async fn login_and_logout() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    assert_eq!(
        rig.post("/api/auth/logout", json!({})).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        rig.get("/api/sources").await.status(),
        StatusCode::UNAUTHORIZED
    );

    let bad = rig
        .post(
            "/api/auth/login",
            json!({"username": "admin", "password": "wrong password"}),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        bad.json::<Value>().await.unwrap(),
        json!({"error": "wrong username or password"})
    );
    assert_eq!(
        rig.post(
            "/api/auth/login",
            json!({"username": "admin", "password": PASSWORD})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(rig.get("/api/sources").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn what_needs_a_login_needs_one_and_a_public_wallboard_opens_only_the_read_only_part() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    rig.post("/api/auth/logout", json!({})).await;

    let denied = rig.get("/api/snapshot").await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        denied.json::<Value>().await.unwrap(),
        json!({"error": "login required", "auth": true})
    );

    rig.post(
        "/api/auth/login",
        json!({"username": "admin", "password": PASSWORD}),
    )
    .await;
    assert_eq!(
        rig.send(
            reqwest::Method::PUT,
            "/api/auth/public-view",
            json!({"enabled": true})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let no_csrf = rig
        .http
        .put(rig.url("/api/settings"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        no_csrf.status(),
        StatusCode::FORBIDDEN,
        "changing something needs the header too"
    );
    rig.post("/api/auth/logout", json!({})).await;

    for open in [
        "/api/snapshot",
        "/api/settings",
        "/api/alerts",
        "/api/uptime",
        "/api/info",
    ] {
        assert_eq!(
            rig.get(open).await.status(),
            StatusCode::OK,
            "{open} is read-only data"
        );
    }
    for closed in ["/api/sources"] {
        assert_eq!(
            rig.get(closed).await.status(),
            StatusCode::UNAUTHORIZED,
            "{closed} shows source details"
        );
    }
    assert_eq!(
        rig.post(
            "/api/sources",
            json!({"name": "x", "type": "Docker Swarm (agent)", "hubUrl": "http://h"})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_added_source_gives_the_manifest_once_and_never_shows_its_token_again() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let info: Value = rig.get("/api/info").await.json().await.unwrap();
    assert_eq!(
        info,
        json!({"demo": false, "sourceTypes": ["Docker (agent)", "Docker Swarm (agent)", "Kubernetes (agent)"], "version": hermes_hub::version::VERSION})
    );

    let (id, token) = rig.add_source("Kubernetes (agent)").await;
    assert_eq!((id.len(), token.len()), (13, 48));
    let list = rig.get("/api/sources").await.text().await.unwrap();
    assert!(!list.contains(&token) && list.contains(&id), "{list}");
    let listed: Value = serde_json::from_str(&list).unwrap();
    assert_eq!(
        (listed[0]["state"].as_str(), listed[0]["info"].as_str()),
        (Some("pending"), Some("waiting for the agent to report"))
    );

    let bad = rig
        .post(
            "/api/sources",
            json!({"name": "x", "type": "Nomad", "hubUrl": "http://h"}),
        )
        .await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
    assert!(
        bad.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("not supported yet")
    );
    assert_eq!(
        rig.post(
            "/api/sources",
            json!({"name": "x", "type": "Docker Swarm (agent)"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST,
        "the hub address is needed for the manifest"
    );
    assert_eq!(
        rig.post(
            "/api/sources",
            json!({"name": " ", "type": "Docker Swarm (agent)", "hubUrl": "h"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn an_agent_over_grpc_shows_up_and_its_changes_reach_the_browser_live() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;

    let mut sse = rig.get("/api/stream").await;
    assert_eq!(sse.headers()["content-type"], "text/event-stream");
    assert_eq!(
        sse.headers()["cache-control"],
        "no-cache",
        "the stream keeps its own header"
    );
    let first = String::from_utf8(sse.chunk().await.unwrap().unwrap().to_vec()).unwrap();
    assert_eq!(
        first.trim(),
        r#"data: {"type":"snapshot","nodes":[],"edges":[]}"#
    );

    let agent = rig.agent(&id, &token);
    agent.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(agent.clone().run());

    // the snapshot reaches the browser that was already listening
    let mut seen = String::new();
    while !seen.contains(r#""type":"snapshot","nodes":[{"#) {
        seen.push_str(&String::from_utf8_lossy(
            &tokio::time::timeout(Duration::from_secs(5), sse.chunk())
                .await
                .expect("no event for 5 s")
                .unwrap()
                .unwrap(),
        ));
    }
    assert!(
        seen.contains(&format!(r#""id":"{id}""#))
            && seen.contains(r#""kind":"cluster""#)
            && seen.contains(r#""provider":"kubernetes""#),
        "{seen}"
    );

    let snap: Value = rig.get("/api/snapshot").await.json().await.unwrap();
    assert_eq!(snap["nodes"][0]["own"], "ok");
    eventually("the source is connected", || async {
        rig.get("/api/sources").await.json::<Value>().await.unwrap()[0]["state"] == "connected"
    })
    .await;

    agent.status(&id, Own::Crit, "unreachable");
    let mut seen = String::new();
    while !seen.contains(r#""type":"status""#) {
        seen.push_str(&String::from_utf8_lossy(
            &tokio::time::timeout(Duration::from_secs(5), sse.chunk())
                .await
                .expect("no event for 5 s")
                .unwrap()
                .unwrap(),
        ));
    }
    assert!(
        seen.contains(r#""reason":"unreachable""#) && seen.contains(r#""own":"crit""#),
        "{seen}"
    );
}

#[tokio::test]
async fn an_agent_with_the_wrong_token_is_turned_away_and_a_deleted_source_loses_its_nodes() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;

    let intruder = rig.agent(&id, "not-the-token");
    intruder.set_topology(vec![cluster("intruder")], vec![]);
    let task = tokio::spawn(intruder.run());
    tokio::time::sleep(Duration::from_millis(400)).await;
    task.abort();
    assert_eq!(
        rig.get("/api/snapshot")
            .await
            .json::<Value>()
            .await
            .unwrap()["nodes"],
        json!([])
    );

    let agent = rig.agent(&id, &token);
    agent.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(agent.run());
    eventually("the cluster appears", || async {
        rig.get("/api/snapshot")
            .await
            .json::<Value>()
            .await
            .unwrap()["nodes"]
            .as_array()
            .unwrap()
            .len()
            == 1
    })
    .await;

    assert_eq!(
        rig.send(
            reqwest::Method::DELETE,
            &format!("/api/sources/{id}"),
            json!({})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        rig.get("/api/snapshot")
            .await
            .json::<Value>()
            .await
            .unwrap()["nodes"],
        json!([])
    );
    assert_eq!(
        rig.get("/api/sources").await.json::<Value>().await.unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn the_agents_have_no_rest_endpoint_any_more() {
    let rig = Rig::start().await;
    let r = rig
        .http
        .post(rig.url("/api/agent/events"))
        .bearer_auth("anything")
        .json(&json!({"events": []}))
        .send()
        .await
        .unwrap();
    // no route: the request falls through to the static files, which refuse a POST
    assert!(r.status().is_client_error(), "{}", r.status());
    assert_eq!(
        rig.get("/api/snapshot").await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn settings_are_stored_broadcast_and_validated() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    assert_eq!(rig.get("/api/settings").await.text().await.unwrap(), "{}");

    let mut sse = rig.get("/api/stream").await;
    sse.chunk().await.unwrap(); // the snapshot
    let bad = rig
        .http
        .put(rig.url("/api/settings"))
        .header("X-Requested-With", "hermes")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    let settings = json!({"rules": [{"id": "volume-usage", "enabled": true, "value": 70, "crit": 90}], "theme": "cz"});
    assert_eq!(
        rig.send(reqwest::Method::PUT, "/api/settings", settings.clone())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        rig.get("/api/settings")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        settings
    );
    let event = String::from_utf8(
        tokio::time::timeout(Duration::from_secs(5), sse.chunk())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let event: Value = serde_json::from_str(event.trim().strip_prefix("data: ").unwrap()).unwrap();
    assert_eq!(event, json!({"type": "settings", "settings": settings}));
}

#[tokio::test]
async fn alerts_and_uptime_are_served_and_an_unknown_alert_cannot_be_acknowledged() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    assert_eq!(
        rig.get("/api/alerts?limit=5")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!([])
    );
    assert_eq!(
        rig.get("/api/uptime?span=60&buckets=4")
            .await
            .json::<Value>()
            .await
            .unwrap(),
        json!({})
    );
    let r = rig.post("/api/alerts/999/ack", json!({})).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        rig.post("/api/alerts/abc/ack", json!({})).await.status(),
        StatusCode::NOT_FOUND
    );
    let _ = rig.addr;
}

#[tokio::test]
async fn the_admin_sees_the_version_of_every_agent_and_which_ones_can_be_updated() {
    let rig = Rig::start().await; // its manifests install agent 1.2.3
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;

    let before: Value = rig.get("/api/sources").await.json().await.unwrap();
    assert_eq!(
        (
            before[0]["agents"].clone(),
            before[0]["expectedAgent"].clone()
        ),
        (json!([]), json!("1.2.3")),
        "no agent yet, and the version to install is known"
    );

    let agent = rig.agent(&id, &token); // says it is 1.0.0
    agent.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(agent.run());
    eventually("the agent shows its version", || async {
        let s: Value = rig.get("/api/sources").await.json().await.unwrap();
        s[0]["agents"][0]["version"] == "1.0.0"
    })
    .await;
    let s: Value = rig.get("/api/sources").await.json().await.unwrap();
    let a = &s[0]["agents"][0];
    assert_eq!(
        (
            &a["id"],
            &a["collector"],
            &a["outdated"],
            &a["protocolOutdated"]
        ),
        (&json!("pod-1"), &json!("node"), &json!(true), &json!(false)),
        "1.0.0 is older than the 1.2.3 the manifests install, but it speaks the hub's own wire protocol"
    );
    assert!(a["seenAgo"].as_u64().unwrap() < 5);
}

/// An agent that can change its own image: it records what it was asked, and answers as told.
struct FakeUpgrader {
    asked: Mutex<Vec<String>>,
    fail: Option<&'static str>,
}

#[hermes_agentkit::async_trait]
impl IUpgrader for FakeUpgrader {
    async fn upgrade(&self, version: &str) -> anyhow::Result<UpgradeOutcome> {
        self.asked.lock().unwrap().push(version.into());
        match self.fail {
            Some(why) => anyhow::bail!(why),
            None => Ok(UpgradeOutcome::Started),
        }
    }
}

#[tokio::test]
async fn the_dashboard_changes_the_agent_version_through_the_agent_and_tells_done_from_failed() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;
    let upgrade = |version: &'static str| {
        let path = format!("/api/sources/{id}/upgrade");
        let rig = &rig;
        async move { rig.post(&path, json!({"version": version})).await }
    };
    let source = || async { rig.get("/api/sources").await.json::<Value>().await.unwrap() };

    // an agent that was installed read-only cannot be told
    let plain = rig.agent(&id, &token);
    plain.set_topology(vec![cluster(&id)], vec![]);
    let plain = tokio::spawn(plain.run());
    eventually("the agent is connected", || async {
        source().await[0]["agents"][0]["version"] == "1.0.0"
    })
    .await;
    assert_eq!(source().await[0]["canUpgrade"], false);
    assert_eq!(upgrade("1.0.5").await.status(), StatusCode::CONFLICT);
    plain.abort();

    // one that can: the hub tells it the version, and only the version
    let fake = Arc::new(FakeUpgrader {
        asked: Mutex::default(),
        fail: None,
    });
    let capable = rig.agent(&id, &token).with_upgrader(fake.clone());
    capable.set_topology(vec![cluster(&id)], vec![]);
    let capable = tokio::spawn(capable.run());
    eventually("the hub knows it can upgrade", || async {
        source().await[0]["canUpgrade"] == true
    })
    .await;
    assert_eq!(upgrade("nonsense").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        rig.post("/api/sources/nope/upgrade", json!({"version": "1.0.5"}))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(upgrade("1.0.5").await.status(), StatusCode::ACCEPTED);
    eventually("the agent was asked", || async {
        *fake.asked.lock().unwrap() == ["1.0.5"]
    })
    .await;
    assert_eq!(
        source().await[0]["upgrade"],
        json!({"version": "1.0.5", "state": "pending", "message": ""})
    );

    // the cluster replaced it: the new agent says the new version, and that is what "done" means
    capable.abort();
    let replaced = rig.agent_version(&id, &token, "1.0.5");
    replaced.set_topology(vec![cluster(&id)], vec![]);
    let replaced = tokio::spawn(replaced.run());
    eventually("done", || async {
        source().await[0]["upgrade"]["state"] == "done"
    })
    .await;
    replaced.abort();

    // an agent that cannot do it says why, and that is shown
    let refusing = rig
        .agent_version(&id, &token, "1.0.5")
        .with_upgrader(Arc::new(FakeUpgrader {
            asked: Mutex::default(),
            fail: Some("the registry refused the pull"),
        }));
    refusing.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(refusing.run());
    eventually("it can be told again", || async {
        source().await[0]["canUpgrade"] == true
    })
    .await;
    assert_eq!(upgrade("1.0.6").await.status(), StatusCode::ACCEPTED);
    eventually("the failure is shown", || async {
        source().await[0]["upgrade"]["state"] == "failed"
    })
    .await;
    let failed = source().await;
    assert_eq!(
        failed[0]["upgrade"]["message"],
        "the registry refused the pull"
    );
}

#[tokio::test]
async fn the_communication_log_is_off_until_an_admin_turns_it_on_and_never_shows_a_token() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (id, token) = rig.add_source("Docker Swarm (agent)").await;
    let log = |query: &str| {
        let url = format!("/api/comm-log{query}");
        let rig = &rig;
        async move { rig.get(&url).await.json::<Value>().await.unwrap() }
    };

    // off: an agent talks, nothing is kept
    let agent = rig.agent(&id, &token);
    agent.set_topology(vec![cluster(&id)], vec![]);
    let run = tokio::spawn(agent.run());
    eventually("the agent is connected", || async {
        rig.get("/api/sources").await.json::<Value>().await.unwrap()[0]["agents"][0]["version"]
            == "1.0.0"
    })
    .await;
    let v = log("").await;
    assert_eq!(
        (v["enabled"].clone(), v["entries"].as_array().unwrap().len()),
        (json!(false), 0)
    );
    run.abort();

    // on: a reconnect and its snapshot are kept, with their content, and heartbeats only counted
    assert_eq!(
        rig.send(
            reqwest::Method::PUT,
            "/api/comm-log",
            json!({"enabled": true})
        )
        .await
        .status(),
        StatusCode::OK
    );
    let agent = rig.agent(&id, &token);
    agent.set_topology(vec![cluster(&id)], vec![]);
    tokio::spawn(agent.run());
    eventually("the snapshot is logged", || async {
        log("?kind=batch").await["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    })
    .await;
    let v = log("").await;
    let kinds: Vec<&str> = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"connected") && kinds.contains(&"batch"),
        "{kinds:?}"
    );
    assert!(
        v["heartbeats"].as_u64().unwrap() > 0,
        "the empty batches are counted, not listed"
    );
    let batch = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "batch")
        .unwrap();
    assert_eq!(
        (
            batch["source"].clone(),
            batch["agent"].clone(),
            batch["dir"].clone()
        ),
        (json!(id), json!("pod-1"), json!("in"))
    );
    assert!(batch["summary"].as_str().unwrap().contains("snapshot ×1"));
    let entry = log(&format!("/{}", batch["id"])).await;
    assert_eq!(
        entry["body"]["events"][0]["content"]["nodes"][0]["id"],
        json!(id)
    );

    // the token is in nothing the log holds
    let everything = serde_json::to_string(&v).unwrap() + &serde_json::to_string(&entry).unwrap();
    assert!(
        !everything.contains(&token),
        "the agent token must never reach the log"
    );

    // filters, and only "newer than"
    assert!(
        log("?q=nothing-like-this-at-all").await["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !log(&format!("?q={}", id)).await["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let newest = v["newest"].as_u64().unwrap();
    assert!(
        log(&format!("?since={}&kind=connected", newest + 100)).await["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        rig.get("/api/comm-log/99999").await.status(),
        StatusCode::NOT_FOUND
    );

    // turning it off forgets it
    rig.send(
        reqwest::Method::PUT,
        "/api/comm-log",
        json!({"enabled": false}),
    )
    .await;
    let v = log("").await;
    assert_eq!(
        (v["enabled"].clone(), v["entries"].as_array().unwrap().len()),
        (json!(false), 0)
    );
}

#[tokio::test]
async fn the_communication_log_is_for_a_logged_in_admin_only() {
    let rig = Rig::start().await;
    rig.setup_admin(true).await; // even with the public wallboard on
    let anon = Client::new();
    assert_eq!(
        anon.get(rig.url("/api/comm-log"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        anon.get(rig.url("/api/comm-log/1"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let put = anon
        .put(rig.url("/api/comm-log"))
        .header("X-Requested-With", "hermes")
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_source_is_renamed_without_touching_its_token_and_a_name_in_use_is_refused() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    let (first, _) = rig.add_source_named("Docker (agent)", "lab-dockr").await; // a typo
    let (second, _) = rig.add_source_named("Docker (agent)", "lab-b").await;

    let path = |id: &str| format!("/api/sources/{id}");
    let ok = rig
        .send(
            reqwest::Method::PATCH,
            &path(&first),
            json!({"name": "  lab-docker-w1  "}),
        )
        .await;
    assert_eq!(ok.status(), StatusCode::OK);
    let listed: Value = rig.get("/api/sources").await.json().await.unwrap();
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"lab-docker-w1"),
        "{names:?} (the name is trimmed)"
    );

    let taken = rig
        .send(
            reqwest::Method::PATCH,
            &path(&second),
            json!({"name": "LAB-docker-w1"}),
        )
        .await;
    assert_eq!(
        taken.status(),
        StatusCode::CONFLICT,
        "case does not make it another name"
    );
    let empty = rig
        .send(
            reqwest::Method::PATCH,
            &path(&second),
            json!({"name": "   "}),
        )
        .await;
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    let gone = rig
        .send(
            reqwest::Method::PATCH,
            "/api/sources/nope",
            json!({"name": "x"}),
        )
        .await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    let same = rig
        .send(
            reqwest::Method::PATCH,
            &path(&first),
            json!({"name": "lab-docker-w1"}),
        )
        .await;
    assert_eq!(
        same.status(),
        StatusCode::OK,
        "keeping its own name is not a conflict"
    );
}

#[tokio::test]
async fn a_second_source_with_the_name_of_another_is_refused_in_any_case() {
    let rig = Rig::start().await;
    rig.setup_admin(false).await;
    rig.add_source_named("Docker (agent)", "runner-2").await;
    for name in ["runner-2", " RUNNER-2 "] {
        let r = rig
            .post(
                "/api/sources",
                json!({"name": name, "type": "Docker (agent)", "hubUrl": rig.base}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CONFLICT, "{name:?}");
    }
    let listed: Value = rig.get("/api/sources").await.json().await.unwrap();
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "nothing was created by the refused ones"
    );
    let other = rig
        .post(
            "/api/sources",
            json!({"name": "runner-3", "type": "Docker (agent)", "hubUrl": rig.base}),
        )
        .await;
    assert_eq!(other.status(), StatusCode::CREATED);
}
