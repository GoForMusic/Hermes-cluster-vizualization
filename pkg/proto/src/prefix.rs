//! A hub that sits behind a reverse proxy at a sub-path (`HUB_BASE_PATH=/hermes`, the proxy does not strip it) is reached at
//! `https://host/hermes`. tonic keeps only the scheme and the host of an address and sends every call to `/infraviz.v1.Service/Method`,
//! so the path has to be put back in front of each call.

use http::Uri;
use tower_service::Service;

/// `https://host:8443/hermes/` -> (`https://host:8443`, `/hermes`). No path -> an empty prefix.
pub fn split_address(address: &str) -> (String, String) {
    let address = address.trim();
    let after_scheme = address.find("://").map_or(0, |i| i + 3);
    let end = address[after_scheme..]
        .find(['/', '?', '#'])
        .map_or(address.len(), |i| after_scheme + i);
    let path = address[end..].split(['?', '#']).next().unwrap_or_default();
    (
        address[..end].to_string(),
        path.trim_end_matches('/').to_string(),
    )
}

/// Wraps a gRPC channel and puts `prefix` in front of the path of every call. An empty prefix changes nothing.
#[derive(Clone)]
pub struct PathPrefix<S> {
    inner: S,
    prefix: String,
}

impl<S> PathPrefix<S> {
    pub fn new(inner: S, prefix: impl Into<String>) -> Self {
        Self {
            inner,
            prefix: prefix.into(),
        }
    }
}

impl<S, B> Service<http::Request<B>> for PathPrefix<S>
where
    S: Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        if !self.prefix.is_empty() {
            let mut parts = req.uri().clone().into_parts();
            let path = parts.path_and_query.as_ref().map_or("/", |pq| pq.as_str());
            // the call's own path always starts with `/`; a prefix that cannot be joined into a valid URI leaves the call as it was
            if let Ok(joined) = format!("{}{path}", self.prefix).parse() {
                parts.path_and_query = Some(joined);
                if let Ok(uri) = Uri::from_parts(parts) {
                    *req.uri_mut() = uri;
                }
            }
        }
        self.inner.call(req)
    }
}

#[cfg(test)]
#[path = "../tests/unit/prefix.rs"]
mod tests;
