//! Reading the log of a container when the hub asks. The hub sends a `LogRequest` over the stream the agent keeps open; the agent reads the
//! log where it lives (the Kubernetes API, the Docker engine) and streams the lines back. Nothing is kept: a viewer that leaves cancels it.
//!
//! What may be read is decided when the agent is installed (the manifest gives it the right, and sets `LOGS=1`), and only then does its hello say
//! `logs`: an agent that was not installed to will say so, never read.

use std::fmt;
use std::sync::Arc;

use tokio::sync::mpsc::Sender;

/// The most lines asked for at the start, whatever the hub says.
pub const MAX_TAIL: u32 = 5000;
/// A chunk to the hub is at most this many lines or bytes (the hub accepts messages of a few MiB).
pub const CHUNK_LINES: usize = 500;
pub const CHUNK_BYTES: usize = 256 * 1024;
/// A line longer than this is cut: a log with no line breaks is not a line, and one message must not be a megabyte.
pub const MAX_LINE: usize = 16 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    pub namespace: String,
    pub name: String,
    pub container: String,
    pub tail: u32,
    pub follow: bool,
    pub previous: bool,
    pub since_seconds: u32,
    pub timestamps: bool,
    pub task: String,
}

#[derive(Debug)]
pub enum LogError {
    /// This agent is not the one to ask for this workload: the hub tries another.
    NotHere(String),
    /// It could not be read, and why, for the person looking.
    Failed(String),
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotHere(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

impl From<anyhow::Error> for LogError {
    fn from(e: anyhow::Error) -> Self {
        Self::Failed(format!("{e:#}"))
    }
}

/// Reads logs where this agent lives.
#[tonic::async_trait]
pub trait ILogs: Send + Sync {
    /// Sends the lines of the log into `out`, in order, in batches. Returns when the log ended (a log that is not followed ends at its last
    /// line) or failed. `out` closing means the viewer left: stop at once.
    async fn read(&self, query: LogQuery, out: Sender<Vec<String>>) -> Result<(), LogError>;
}

pub type SharedLogs = Arc<dyn ILogs>;

/// Cuts a line that is too long, on a character boundary, and says so.
pub fn clip(mut line: String) -> String {
    if line.len() > MAX_LINE {
        let mut cut = MAX_LINE;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
        line.push_str(" … [line cut]");
    }
    line
}

/// Makes batches out of lines as they are read: a batch goes out when it is `CHUNK_LINES` long or `CHUNK_BYTES` big, and what is left when
/// the reader stops (`flush`). The reader asks the batcher; the batcher asks the channel, which is what slows a fast log down.
pub struct Batcher {
    out: Sender<Vec<String>>,
    lines: Vec<String>,
    bytes: usize,
}

impl Batcher {
    pub fn new(out: Sender<Vec<String>>) -> Self {
        Self {
            out,
            lines: Vec::new(),
            bytes: 0,
        }
    }

    /// `false`: the viewer left.
    pub async fn push(&mut self, line: String) -> bool {
        self.bytes += line.len();
        self.lines.push(clip(line));
        if self.lines.len() >= CHUNK_LINES || self.bytes >= CHUNK_BYTES {
            return self.flush().await;
        }
        true
    }

    pub async fn flush(&mut self) -> bool {
        if self.lines.is_empty() {
            return true;
        }
        self.bytes = 0;
        self.out.send(std::mem::take(&mut self.lines)).await.is_ok()
    }
}

#[cfg(test)]
#[path = "../tests/unit/logs.rs"]
mod tests;
