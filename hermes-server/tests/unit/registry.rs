use super::*;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode as Code};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

fn config(base: &str, auth: &str) -> RegistryConfig {
    RegistryConfig {
        url: base.into(),
        project: "acm".into(),
        auth: auth.into(),
        username: "robot".into(),
        secret: "s3cret".into(),
        ..Default::default()
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// A registry that wants basic auth: robot / s3cret.
fn basic_registry() -> Router {
    async fn guard(headers: HeaderMap, req: Request) -> axum::response::Response {
        let ok = headers
            .get("authorization")
            .is_some_and(|v| v == "Basic cm9ib3Q6czNjcmV0");
        if !ok {
            return (
                Code::UNAUTHORIZED,
                [("www-authenticate", "Basic realm=\"r\"")],
            )
                .into_response();
        }
        if req.uri().path() == "/v2/" {
            return Json(json!({})).into_response();
        }
        Json(json!({"name": "acm/hermes-agent-linux", "tags": ["1.0.0", "latest", "1.10.0", "1.2.0"]})).into_response()
    }
    Router::new().fallback(get(guard))
}

#[tokio::test]
async fn basic_auth_registry_lists_versions_newest_first() {
    let base = serve(basic_registry()).await;
    let c = RegistryClient::new();
    let cfg = config(&base, "basic");
    c.check(&cfg).await.unwrap();
    assert_eq!(
        c.tags(&cfg, "hermes-agent-linux").await.unwrap(),
        ["1.10.0", "1.2.0", "1.0.0", "latest"]
    );
}

#[tokio::test]
async fn wrong_password_is_reported_as_such() {
    let base = serve(basic_registry()).await;
    let mut cfg = config(&base, "basic");
    cfg.secret = "nope".into();
    let e = RegistryClient::new()
        .check(&cfg)
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused this user and password"), "{e}");
    cfg.auth = "none".into();
    let e = RegistryClient::new()
        .check(&cfg)
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("wants a login"), "{e}");
}

/// Docker Hub / Harbor / GHCR style: the registry answers 401 with a Bearer challenge, and a token service hands out the token.
#[tokio::test]
async fn bearer_registry_gets_a_token_from_its_login_service() {
    async fn token(
        headers: HeaderMap,
        axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
    ) -> axum::response::Response {
        let authed = headers
            .get("authorization")
            .is_some_and(|v| v == "Basic cm9ib3Q6czNjcmV0");
        if authed
            && q.get("scope")
                .is_some_and(|s| s == "repository:acm/hermes-agent-linux:pull")
            && q.get("service").is_some_and(|s| s == "reg")
        {
            Json(json!({"token": "T0K"})).into_response()
        } else {
            Code::UNAUTHORIZED.into_response()
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let realm = format!("{base}/token");
    let registry = move |headers: HeaderMap| {
        let realm = realm.clone();
        async move {
            if headers
                .get("authorization")
                .is_some_and(|v| v == "Bearer T0K")
            {
                Json(json!({"tags": ["2.0.0", "1.9.9"]})).into_response()
            } else {
                (
                    Code::UNAUTHORIZED,
                    [(
                        "www-authenticate",
                        format!(
                            "Bearer realm=\"{realm}\",service=\"reg\",scope=\"repository:x:pull\""
                        ),
                    )],
                )
                    .into_response()
            }
        }
    };
    let app = Router::new()
        .route("/token", get(token))
        .fallback(get(registry));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let cfg = config(&base, "basic");
    assert_eq!(
        RegistryClient::new()
            .tags(&cfg, "hermes-agent-linux")
            .await
            .unwrap(),
        ["2.0.0", "1.9.9"]
    );
}

#[tokio::test]
async fn a_missing_repository_and_an_unreachable_registry_are_explained() {
    let base = serve(Router::new().fallback(get(|req: Request| async move {
        if req.uri().path() == "/v2/" {
            Json(json!({})).into_response()
        } else {
            Code::NOT_FOUND.into_response()
        }
    })))
    .await;
    let cfg = config(&base, "none");
    let c = RegistryClient::new();
    c.check(&cfg).await.unwrap();
    let e = c
        .tags(&cfg, "hermes-agent-linux")
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("acm/hermes-agent-linux is not in"), "{e}");
    let e = c
        .check(&config("http://127.0.0.1:1", "none"))
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("cannot connect"), "{e}");
    assert!(c.check(&RegistryConfig::default()).await.is_err());
}

#[test]
fn the_challenge_is_read_even_with_a_comma_inside_a_scope() {
    let c = parse_challenge(r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:a/b:pull,push""#).unwrap();
    assert_eq!(
        (c.realm.as_str(), c.service.as_str(), c.scope.as_str()),
        (
            "https://auth.docker.io/token",
            "registry.docker.io",
            "repository:a/b:pull,push"
        )
    );
    assert!(parse_challenge("Basic realm=\"x\"").is_none());
}

#[test]
fn image_names_follow_the_registry() {
    let mut c = config("https://git.example.com/", "none");
    assert_eq!(
        c.image(c.linux_repo()),
        "git.example.com/acm/hermes-agent-linux"
    );
    assert_eq!(c.base(), "https://git.example.com");
    c.url = "http://10.0.0.5:5000".into();
    assert_eq!(
        (c.base().as_str(), c.host()),
        ("http://10.0.0.5:5000", "10.0.0.5:5000")
    );
    c.url = "docker.io".into();
    assert_eq!(
        (c.base().as_str(), c.image("x").as_str()),
        ("https://registry-1.docker.io", "docker.io/acm/x")
    );
}
