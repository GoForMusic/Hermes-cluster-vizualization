use super::*;

#[test]
fn a_query_value_is_percent_encoded() {
    assert_eq!(
        pct_encode(r#"{"label":["a.b/c"]}"#),
        "%7B%22label%22%3A%5B%22a.b%2Fc%22%5D%7D"
    );
    assert_eq!(pct_encode("plain-1_2.3~"), "plain-1_2.3~");
}

#[tokio::test]
async fn an_endpoint_the_platform_cannot_open_says_so_when_used() {
    #[cfg(unix)]
    let engine = Engine::new(connector_for("npipe:////./pipe/docker_engine"));
    #[cfg(not(unix))]
    let engine = Engine::new(connector_for("/var/run/docker.sock"));
    let error = engine.get::<serde_json::Value>("/info").await.unwrap_err();
    assert!(
        format!("{error:#}").contains("only on Windows")
            || format!("{error:#}").contains("do not exist"),
        "{error:#}"
    );
}

#[tokio::test]
async fn a_socket_that_is_not_there_is_an_error_not_a_hang() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(connector_for(&format!(
        "unix://{}/none.sock",
        dir.path().display()
    )));
    let error = engine.get::<serde_json::Value>("/info").await.unwrap_err();
    assert!(format!("{error:#}").contains("cannot connect"), "{error:#}");
}
