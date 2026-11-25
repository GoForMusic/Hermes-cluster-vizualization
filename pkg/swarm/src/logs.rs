//! Reading the log of a container or of a Swarm task for the dashboard's log panel, from the Docker engine. A Swarm manager's engine serves
//! the logs of every task of the swarm (`/tasks/{id}/logs`), so one agent on a manager is enough; a worker's agent says it is not the one. A
//! Docker machine on its own serves the logs of its own containers (`/containers/{name}/logs`).

use std::sync::Arc;
use std::time::Duration;

use hermes_agentkit::logs::Batcher;
use hermes_agentkit::{ILogs, LogError, LogQuery};
use http_body_util::BodyExt;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

use crate::engine::{Engine, pct_encode};

/// A followed log with nothing new for this long sends what it has.
const FLUSH_AFTER: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A node of a swarm: the workloads are tasks, read through a manager.
    Swarm,
    /// A Docker machine of its own: the workloads are containers.
    Docker,
}

pub struct DockerLogs {
    engine: Arc<Engine>,
    mode: Mode,
}

impl DockerLogs {
    pub fn new(engine: Arc<Engine>, mode: Mode) -> Self {
        Self { engine, mode }
    }

    /// The task the log is of: the one asked for, or, for the log before it, the task that ran this slot of the service before it.
    async fn task(&self, q: &LogQuery) -> std::result::Result<String, LogError> {
        if q.task.is_empty() {
            return Err(LogError::Failed(
                "the task is not known: reload the page".into(),
            ));
        }
        let asked = pct_encode(&q.task);
        let this: Value = self
            .engine
            .get(&format!("/tasks/{asked}"))
            .await
            .map_err(not_here)?;
        if !q.previous {
            return Ok(q.task.clone());
        }
        let (service, slot, created) = (
            str_of(&this, "ServiceID"),
            this["Slot"].as_i64(),
            str_of(&this, "CreatedAt"),
        );
        let filter = pct_encode(&format!(r#"{{"service":["{service}"]}}"#));
        let all: Vec<Value> = self
            .engine
            .get(&format!("/tasks?filters={filter}"))
            .await
            .map_err(not_here)?;
        // the latest task of the same slot that began before this one (a global service has no slot: its nodes tell them apart)
        let node = str_of(&this, "NodeID");
        all.iter()
            .filter(|t| str_of(t, "ID") != q.task)
            .filter(|t| {
                t["Slot"].as_i64() == slot
                    && (slot.is_some_and(|s| s != 0) || str_of(t, "NodeID") == node)
            })
            .filter(|t| str_of(t, "CreatedAt") < created)
            .max_by_key(|t| str_of(t, "CreatedAt"))
            .map(|t| str_of(t, "ID"))
            .ok_or_else(|| {
                LogError::Failed("this task has no earlier one: it has not been replaced".into())
            })
    }
}

fn str_of(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

/// What the engine said, as the person is told. A worker has no say over tasks; a task or container that is not there is for another agent to find.
fn not_here(e: anyhow::Error) -> LogError {
    let text = format!("{e:#}");
    if text.contains("not a swarm manager") {
        LogError::NotHere("this node is not a swarm manager".into())
    } else if text.contains("404") {
        LogError::NotHere("the engine does not know it (it may be gone)".into())
    } else {
        LogError::Failed(text)
    }
}

#[hermes_agentkit::async_trait]
impl ILogs for DockerLogs {
    async fn read(&self, q: LogQuery, out: Sender<Vec<String>>) -> Result<(), LogError> {
        let target = match self.mode {
            Mode::Swarm => format!("/tasks/{}/logs", pct_encode(&self.task(&q).await?)),
            Mode::Docker => {
                if q.previous {
                    return Err(LogError::Failed(
                        "Docker keeps one log for a container, across its restarts: there is no earlier one".into(),
                    ));
                }
                if q.name.is_empty() {
                    return Err(LogError::Failed("which container?".into()));
                }
                format!("/containers/{}/logs", pct_encode(&q.name))
            }
        };
        let mut query = format!("stdout=1&stderr=1&tail={}", q.tail);
        if q.follow {
            query.push_str("&follow=1");
        }
        if q.timestamps {
            query.push_str("&timestamps=1");
        }
        if q.since_seconds > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            query.push_str(&format!(
                "&since={}",
                now.saturating_sub(u64::from(q.since_seconds))
            ));
        }
        let mut body = self
            .engine
            .open(&format!("{target}?{query}"))
            .await
            .map_err(not_here)?;
        let mut decoder = Decoder::default();
        let mut batch = Batcher::new(out);
        loop {
            match tokio::time::timeout(FLUSH_AFTER, body.frame()).await {
                Ok(Some(Ok(frame))) => {
                    let Ok(data) = frame.into_data() else {
                        continue;
                    };
                    for line in decoder.push(&data) {
                        if !batch.push(line).await {
                            return Ok(()); // the viewer left
                        }
                    }
                }
                Ok(Some(Err(e))) => {
                    batch.flush().await;
                    return Err(LogError::Failed(format!("the log stopped: {e}")));
                }
                Ok(None) => {
                    if let Some(rest) = decoder.finish() {
                        batch.push(rest).await;
                    }
                    batch.flush().await;
                    return Ok(());
                }
                Err(_) => {
                    if !batch.flush().await {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Turns what the engine sends into lines. A container with no terminal sends frames (an 8-byte header: the stream, three zeros, the length
/// of the payload as 32 bits big-endian), a container with one sends the text as it is; the first bytes say which.
#[derive(Default)]
pub struct Decoder {
    kind: Option<Kind>,
    /// bytes not yet a whole frame (framed) or header
    pending: Vec<u8>,
    /// the text of the line being written
    line: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Framed,
    Plain,
}

impl Decoder {
    pub fn push(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        self.pending.extend_from_slice(data);
        if self.kind.is_none() {
            if self.pending.len() < 4 {
                return lines;
            }
            let p = &self.pending;
            self.kind = Some(if p[0] <= 2 && p[1] == 0 && p[2] == 0 && p[3] == 0 {
                Kind::Framed
            } else {
                Kind::Plain
            });
        }
        match self.kind {
            Some(Kind::Plain) => {
                let text = std::mem::take(&mut self.pending);
                self.text(&text, &mut lines);
            }
            _ => loop {
                if self.pending.len() < 8 {
                    break;
                }
                let size = u32::from_be_bytes([
                    self.pending[4],
                    self.pending[5],
                    self.pending[6],
                    self.pending[7],
                ]) as usize;
                if self.pending.len() < 8 + size {
                    break;
                }
                let payload: Vec<u8> = self.pending.drain(..8 + size).skip(8).collect();
                self.text(&payload, &mut lines);
            },
        }
        lines
    }

    fn text(&mut self, bytes: &[u8], lines: &mut Vec<String>) {
        for &b in bytes {
            if b == b'\n' {
                lines.push(self.take_line());
            } else {
                self.line.push(b);
            }
        }
    }

    fn take_line(&mut self) -> String {
        if self.line.last() == Some(&b'\r') {
            self.line.pop();
        }
        String::from_utf8_lossy(&std::mem::take(&mut self.line)).into_owned()
    }

    /// The last line, when the log did not end with a line break.
    pub fn finish(&mut self) -> Option<String> {
        // a plain log that ended mid-line has its tail in `pending`
        if self.kind == Some(Kind::Plain) && !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.line.extend_from_slice(&rest);
        }
        (!self.line.is_empty()).then(|| self.take_line())
    }
}

#[cfg(test)]
#[path = "../tests/unit/logs.rs"]
mod tests;
