//! The hub terminating TLS itself (`HUB_TLS_CERT`/`HUB_TLS_KEY` in `main.rs`), instead of sitting behind a proxy like the rest
//! of `e2e.rs` assumes. A self-signed cert stands in for a real one; REST and an agent's gRPC both have to work over it, on
//! the one port, which is the point of using `axum_server::bind_rustls` instead of a second listener.

use std::net::SocketAddr;
use std::time::Duration;

use axum_server::tls_rustls::RustlsConfig;
use hermes_agentkit::{Config, ISink, Timing, Uplink};
use hermes_hub::app::{Hub, Settings};
use hermes_hub::database::Repositories;
use hermes_proto::v1::{Node, NodeKind, Own, Provider};
use reqwest::{Certificate, Client, StatusCode};
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

struct TlsRig {
    base: String,
    http: Client,
    cert_pem: Vec<u8>,
    _web: tempfile::TempDir,
    _certs: tempfile::TempDir,
}

impl TlsRig {
    async fn start() -> Self {
        let cert =
            rcgen::generate_simple_self_signed(["localhost".to_string(), "127.0.0.1".to_string()])
                .expect("self-signed cert for localhost/127.0.0.1");
        let cert_pem = cert.cert.pem().into_bytes();
        let key_pem = cert.signing_key.serialize_pem();
        let certs = tempfile::tempdir().unwrap();
        let cert_path = certs.path().join("cert.pem");
        let key_path = certs.path().join("key.pem");
        std::fs::write(&cert_path, &cert_pem).unwrap();
        std::fs::write(&key_path, &key_pem).unwrap();

        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("index.html"), "<h1>infraviz</h1>").unwrap();

        let hub = Hub::new(
            Repositories::sqlite_in_memory().unwrap(),
            Settings {
                web: web.path().into(),
                agent_image: "registry.example.com/agent:1.2.3".into(),
                agent_image_windows: String::new(),
                agent_pull_secret: String::new(),
                tls_enabled: true,
            },
        );
        hub.spawn_background();
        let service = hub
            .router()
            .into_make_service_with_connect_info::<SocketAddr>();
        let config = RustlsConfig::from_pem_file(&cert_path, &key_path)
            .await
            .expect("the generated cert/key load");
        let handle = axum_server::Handle::<SocketAddr>::new();
        tokio::spawn({
            let handle = handle.clone();
            async move {
                let _ = axum_server::bind_rustls("127.0.0.1:0".parse().unwrap(), config)
                    .handle(handle)
                    .serve(service)
                    .await;
            }
        });
        let addr = handle
            .listening()
            .await
            .expect("the TLS listener binds a port");

        let http = Client::builder()
            .cookie_store(true)
            .add_root_certificate(Certificate::from_pem(&cert_pem).unwrap())
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        Self {
            base: format!("https://{addr}"),
            http,
            cert_pem,
            _web: web,
            _certs: certs,
        }
    }

    async fn post(&self, path: &str, body: Value) -> reqwest::Response {
        self.http
            .post(format!("{}{path}", self.base))
            .header("X-Requested-With", "hermes")
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.http
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    async fn setup_admin(&self) {
        let r = self
            .post(
                "/api/auth/setup",
                json!({"username": "admin", "password": PASSWORD, "publicView": false}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
    }

    /// Adds an agent source and returns its id, token and a CA file the agent can trust this hub's cert with.
    async fn add_source(&self) -> (String, String, std::path::PathBuf) {
        let r = self
            .post(
                "/api/sources",
                json!({"name": "lab", "type": "Docker Swarm (agent)", "hubUrl": self.base}),
            )
            .await;
        assert_eq!(r.status(), StatusCode::CREATED);
        let body: Value = r.json().await.unwrap();
        let token = body["install"]
            .as_str()
            .unwrap()
            .lines()
            .find_map(|l| l.trim().strip_prefix("TOKEN: \""))
            .unwrap()
            .trim_end_matches('"')
            .to_string();
        let ca_path = self._certs.path().join("cert.pem");
        std::fs::write(&ca_path, &self.cert_pem).unwrap();
        (body["id"].as_str().unwrap().to_string(), token, ca_path)
    }
}

/// Polls until `check` holds, or fails after a few seconds (same budget as `e2e.rs`'s `eventually`, duplicated rather than
/// shared across two test binaries for one helper).
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
async fn rest_and_login_work_over_the_hub_s_own_tls() {
    let rig = TlsRig::start().await;
    let health = rig.get("/api/health").await;
    assert_eq!(health.status(), StatusCode::OK);

    let setup = rig
        .post(
            "/api/auth/setup",
            json!({"username": "admin", "password": PASSWORD, "publicView": false}),
        )
        .await;
    assert_eq!(setup.status(), StatusCode::CREATED);
    assert!(
        setup.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("; Secure"),
        "the hub knows it is serving TLS itself, no X-Forwarded-Proto needed"
    );
}

#[tokio::test]
async fn an_agent_connects_over_the_hub_s_own_tls() {
    let rig = TlsRig::start().await;
    rig.setup_admin().await;
    let (id, token, ca_path) = rig.add_source().await;

    let env = [
        ("HUB_URL", rig.base.as_str()),
        ("SOURCE_ID", id.as_str()),
        ("SOURCE_NAME", "lab"),
        ("TOKEN", token.as_str()),
        ("AGENT_ID", "pod-1"),
        ("AGENT_HOST", ""),
        ("HUB_CA_FILE", ca_path.to_str().unwrap()),
    ];
    let cfg = Config::from_lookup("node", "1.0.0", |k| {
        env.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| (*v).to_string())
    })
    .unwrap();
    let agent = Uplink::new(&cfg).unwrap().with_timing(Timing {
        flush: Duration::from_millis(30),
        backoff_min: Duration::from_millis(30),
        backoff_max: Duration::from_millis(100),
    });
    agent.set_topology(
        vec![Node {
            id: id.clone(),
            kind: NodeKind::Cluster.into(),
            name: "lab".into(),
            provider: Provider::Kubernetes.into(),
            own: Own::Ok.into(),
            ..Default::default()
        }],
        vec![],
    );
    tokio::spawn(agent.run());

    eventually("the agent's hello reaches the hub over TLS", || async {
        let s: Value = rig.get("/api/sources").await.json().await.unwrap();
        s[0]["agents"][0]["id"] == "pod-1"
    })
    .await;
}
