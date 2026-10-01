//! A client for the Docker Engine API: HTTP over the engine's local socket (a unix socket on Linux, a named pipe on Windows). One connection
//! per request: the engine answers quickly and a stats call takes about a second anyway.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::header::HOST;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};

const TIMEOUT: Duration = Duration::from_secs(10);

pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
pub type BoxIo = Box<dyn Io>;
pub type Connecting<'a> = Pin<Box<dyn Future<Output = io::Result<BoxIo>> + Send + 'a>>;

/// Opens a connection to the engine.
pub trait IConnect: Send + Sync {
    fn connect(&self) -> Connecting<'_>;
}

/// The connector for an endpoint: `/var/run/docker.sock` or `unix:///var/run/docker.sock`, or `npipe:////./pipe/docker_engine` on Windows.
/// An endpoint the platform cannot open gives a connector that says so when it is used.
pub fn connector_for(endpoint: &str) -> Arc<dyn IConnect> {
    match endpoint.strip_prefix("npipe://") {
        Some(pipe) => named_pipe(pipe, endpoint),
        None => unix_socket(
            endpoint.strip_prefix("unix://").unwrap_or(endpoint),
            endpoint,
        ),
    }
}

struct Unsupported(String);

impl IConnect for Unsupported {
    fn connect(&self) -> Connecting<'_> {
        Box::pin(async move { Err(io::Error::new(io::ErrorKind::Unsupported, self.0.clone())) })
    }
}

#[cfg(unix)]
struct UnixSocket(std::path::PathBuf);

#[cfg(unix)]
impl IConnect for UnixSocket {
    fn connect(&self) -> Connecting<'_> {
        Box::pin(
            async move { Ok(Box::new(tokio::net::UnixStream::connect(&self.0).await?) as BoxIo) },
        )
    }
}

#[cfg(unix)]
fn unix_socket(path: &str, _endpoint: &str) -> Arc<dyn IConnect> {
    Arc::new(UnixSocket(path.into()))
}

#[cfg(not(unix))]
fn unix_socket(_path: &str, endpoint: &str) -> Arc<dyn IConnect> {
    Arc::new(Unsupported(format!(
        "unix sockets do not exist on this system (endpoint {endpoint:?})"
    )))
}

#[cfg(windows)]
struct NamedPipe(String);

#[cfg(windows)]
impl IConnect for NamedPipe {
    fn connect(&self) -> Connecting<'_> {
        Box::pin(async move {
            // ERROR_PIPE_BUSY: every instance of the pipe is in use for a moment; try again shortly
            const PIPE_BUSY: i32 = 231;
            for _ in 0..10 {
                match tokio::net::windows::named_pipe::ClientOptions::new().open(&self.0) {
                    Ok(pipe) => return Ok(Box::new(pipe) as BoxIo),
                    Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => {
                        tokio::time::sleep(Duration::from_millis(50)).await
                    }
                    Err(e) => return Err(e),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the Docker engine pipe stayed busy",
            ))
        })
    }
}

#[cfg(windows)]
fn named_pipe(pipe: &str, _endpoint: &str) -> Arc<dyn IConnect> {
    Arc::new(NamedPipe(pipe.replace('/', "\\")))
}

#[cfg(not(windows))]
fn named_pipe(_pipe: &str, endpoint: &str) -> Arc<dyn IConnect> {
    Arc::new(Unsupported(format!(
        "named pipes exist only on Windows (endpoint {endpoint:?})"
    )))
}

pub struct Engine {
    connector: Arc<dyn IConnect>,
}

impl Engine {
    pub fn new(connector: Arc<dyn IConnect>) -> Self {
        Self { connector }
    }

    /// `GET path`, the answer read as JSON.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let body = tokio::time::timeout(TIMEOUT, self.request(Method::GET, path, Bytes::new()))
            .await
            .with_context(|| format!("docker {path}: no answer in {}s", TIMEOUT.as_secs()))??;
        serde_json::from_slice(&body)
            .with_context(|| format!("docker {path}: the answer is not what was expected"))
    }

    /// `POST path` with a JSON body; the answer, if any, is ignored except for an error.
    pub async fn post(&self, path: &str, body: &serde_json::Value) -> Result<()> {
        let body = Bytes::from(serde_json::to_vec(body)?);
        tokio::time::timeout(TIMEOUT, self.request(Method::POST, path, body))
            .await
            .with_context(|| format!("docker {path}: no answer in {}s", TIMEOUT.as_secs()))??;
        Ok(())
    }

    async fn request(&self, method: Method, path: &str, body: Bytes) -> Result<Bytes> {
        let io = self
            .connector
            .connect()
            .await
            .context("cannot connect to the Docker engine")?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(io))
            .await
            .context("cannot talk to the Docker engine")?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(HOST, "docker");
        if !body.is_empty() {
            request = request.header(hyper::header::CONTENT_TYPE, "application/json");
        }
        let request = request.body(Full::new(body)).context("bad request")?;
        let response = sender
            .send_request(request)
            .await
            .with_context(|| format!("docker {path}"))?;
        let status = response.status();
        let body = response
            .into_body()
            .collect()
            .await
            .with_context(|| format!("docker {path}"))?
            .to_bytes();
        if !status.is_success() {
            let text = String::from_utf8_lossy(&body[..body.len().min(512)]);
            bail!("docker {path}: {status} {}", text.trim());
        }
        Ok(body)
    }
}

/// Percent-encodes a query value: everything but letters, digits and `-_.~`.
pub fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/engine.rs"]
mod tests;
