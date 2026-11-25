use super::*;
use std::convert::Infallible;
use std::task::{Context, Poll};

#[test]
fn an_address_splits_into_where_to_connect_and_the_path_in_front() {
    for (address, origin, prefix) in [
        ("https://hub.example.com", "https://hub.example.com", ""),
        ("https://hub.example.com/", "https://hub.example.com", ""),
        (
            "https://hub.example.com/hermes",
            "https://hub.example.com",
            "/hermes",
        ),
        ("http://10.0.0.1:8765/a/b/", "http://10.0.0.1:8765", "/a/b"),
        ("http://h:1/hermes?x=1#y", "http://h:1", "/hermes"),
        ("http://h:1?x=1", "http://h:1", ""),
        (" http://h/hermes ", "http://h", "/hermes"),
    ] {
        assert_eq!(
            split_address(address),
            (origin.to_string(), prefix.to_string()),
            "{address}"
        );
    }
}

/// Answers with the URI it was called with.
struct Echo;

impl Service<http::Request<()>> for Echo {
    type Response = String;
    type Error = Infallible;
    type Future = std::future::Ready<Result<String, Infallible>>;
    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }
    fn call(&mut self, req: http::Request<()>) -> Self::Future {
        std::future::ready(Ok(req.uri().to_string()))
    }
}

fn call(prefix: &str, uri: &str) -> String {
    let mut svc = PathPrefix::new(Echo, prefix);
    let req = http::Request::builder().uri(uri).body(()).unwrap();
    futures_now(svc.call(req))
}

fn futures_now(f: std::future::Ready<Result<String, Infallible>>) -> String {
    f.into_inner().unwrap()
}

#[test]
fn the_prefix_goes_in_front_of_the_path_of_every_call() {
    assert_eq!(
        call("/hermes", "http://h:1/infraviz.v1.AgentService/Session"),
        "http://h:1/hermes/infraviz.v1.AgentService/Session"
    );
    assert_eq!(call("", "http://h:1/x/y"), "http://h:1/x/y");
}
