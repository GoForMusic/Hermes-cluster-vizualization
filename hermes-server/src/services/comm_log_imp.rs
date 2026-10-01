//! The agent ↔ hub communication log: what each agent sent and what the hub sent back, kept for a while to answer "what did agent X send
//! and when", "why is pod Y not there", "did the hub ask for a resync". It can show pod names, images and addresses, so it is:
//!
//! * OFF until an admin turns it on, and off again after a restart;
//! * only in memory (a ring: the last `CAPACITY` entries or `KEPT` minutes, whichever is less);
//! * only for a logged-in admin (it is not part of the event stream the public wallboard reads);
//! * without any token: a batch carries none, and the session's own token is never passed in.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde_json::Value;

use crate::model::{CommLogEntry, CommLogRow, CommLogView};

pub const CAPACITY: usize = 500;
pub const KEPT_MINUTES: u32 = 15;
/// A content bigger than this is cut: a first snapshot of a big cluster is megabytes.
const MAX_BODY: usize = 64 * 1024;

/// What a caller hands in. Built only when the log is on (`enabled()`), because the content can be costly to make.
pub struct NewEntry {
    pub ts: i64,
    pub source: String,
    pub source_name: String,
    pub agent: String,
    pub dir: &'static str,
    pub kind: &'static str,
    pub summary: String,
    pub bytes: u64,
    pub body: Value,
}

pub trait ICommLog: Send + Sync {
    fn enabled(&self) -> bool;
    /// Turning it off forgets what was kept.
    fn set_enabled(&self, on: bool);
    fn record(&self, entry: NewEntry);
    /// An empty batch: counted, never listed.
    fn heartbeat(&self);
    /// Newest first, only the ones newer than `since`, optionally of one source / kind / containing `q` anywhere (also in the content).
    fn list(&self, since: u64, source: &str, kind: &str, q: &str, limit: usize) -> CommLogView;
    fn get(&self, id: u64) -> Option<CommLogEntry>;
    fn clear(&self);
}

struct Kept {
    row: CommLogRow,
    body: Value,
    /// Lower case, for searching: the row's words and the content.
    haystack: String,
}

#[derive(Default)]
pub struct CommLogImp {
    on: AtomicBool,
    next: AtomicU64,
    heartbeats: AtomicU64,
    ring: Mutex<VecDeque<Kept>>,
}

impl CommLogImp {
    pub fn new() -> Self {
        Self::default()
    }

    fn ring(&self) -> std::sync::MutexGuard<'_, VecDeque<Kept>> {
        self.ring
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn view(&self, entries: Vec<CommLogRow>) -> CommLogView {
        CommLogView {
            enabled: self.enabled(),
            capacity: u32::try_from(CAPACITY).unwrap_or(u32::MAX),
            minutes: KEPT_MINUTES,
            heartbeats: self.heartbeats.load(Ordering::Relaxed),
            entries,
            newest: self.next.load(Ordering::Relaxed),
        }
    }
}

/// A content too big to keep whole is replaced by its beginning and a note.
fn capped(body: Value) -> Value {
    let text = body.to_string();
    if text.len() <= MAX_BODY {
        return body;
    }
    let mut cut = MAX_BODY;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    serde_json::json!({"truncated": true, "size": text.len(), "beginning": &text[..cut]})
}

impl ICommLog for CommLogImp {
    fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    fn set_enabled(&self, on: bool) {
        let was = self.on.swap(on, Ordering::Relaxed);
        if was && !on {
            self.clear();
        }
        if on && !was {
            self.heartbeats.store(0, Ordering::Relaxed);
        }
    }

    fn record(&self, e: NewEntry) {
        if !self.enabled() {
            return;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let body = capped(e.body);
        let row = CommLogRow {
            id,
            ts: e.ts,
            source: e.source,
            source_name: e.source_name,
            agent: e.agent,
            dir: e.dir.into(),
            kind: e.kind.into(),
            summary: e.summary,
            bytes: e.bytes,
        };
        let haystack = format!(
            "{} {} {} {} {} {}",
            row.source_name, row.source, row.agent, row.kind, row.summary, body
        )
        .to_lowercase();
        let mut ring = self.ring();
        ring.push_back(Kept {
            row,
            body,
            haystack,
        });
        let oldest = e.ts - i64::from(KEPT_MINUTES) * 60_000;
        while ring.len() > CAPACITY || ring.front().is_some_and(|k| k.row.ts < oldest) {
            ring.pop_front();
        }
    }

    fn heartbeat(&self) {
        if self.enabled() {
            self.heartbeats.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn list(&self, since: u64, source: &str, kind: &str, q: &str, limit: usize) -> CommLogView {
        let q = q.trim().to_lowercase();
        let rows = self
            .ring()
            .iter()
            .rev()
            .filter(|k| k.row.id > since)
            .filter(|k| source.is_empty() || k.row.source == source)
            .filter(|k| kind.is_empty() || k.row.kind == kind)
            .filter(|k| q.is_empty() || k.haystack.contains(&q))
            .take(limit)
            .map(|k| k.row.clone())
            .collect();
        self.view(rows)
    }

    fn get(&self, id: u64) -> Option<CommLogEntry> {
        self.ring()
            .iter()
            .find(|k| k.row.id == id)
            .map(|k| CommLogEntry {
                row: k.row.clone(),
                body: k.body.clone(),
            })
    }

    fn clear(&self) {
        self.ring().clear();
    }
}

/// A batch as the log shows it: a few words for the list, and every event with its content for the detail. A snapshot lists at most
/// `SNAPSHOT_NODES` nodes and says how many more there were.
pub fn describe_batch(events: &[crate::services::Event]) -> (String, Value) {
    use crate::services::Event;
    use serde_json::json;
    const SNAPSHOT_NODES: usize = 200;
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    let mut bodies = Vec::new();
    for e in events {
        let (kind, body) = match e {
            Event::Snapshot { nodes, edges } => (
                "snapshot",
                json!({"nodes": nodes.iter().take(SNAPSHOT_NODES).collect::<Vec<_>>(), "moreNodes": nodes.len().saturating_sub(SNAPSHOT_NODES), "edges": edges.len()}),
            ),
            Event::Status { id, own, reason } => ("status", json!({"id": id, "own": own, "reason": reason})),
            Event::Meta { id, patch } => ("meta", json!({"id": id, "patch": patch})),
            Event::Metrics { nodes, edges } => ("metrics", json!({"nodes": nodes, "edges": edges})),
            Event::Report { state, info } => ("report", json!({"state": state, "info": info})),
            Event::Alive(ids) => ("alive", json!({"ids": ids})),
            Event::Contribution(nodes) => ("contribution", json!({"nodes": nodes})),
            Event::Flows(flows) => (
                "flows",
                json!(flows.iter().map(|f| json!({"src": f.src, "dst": f.dst, "servedBy": f.served_by, "port": f.port, "proto": f.proto, "outMbps": f.out_mbps, "inMbps": f.in_mbps})).collect::<Vec<_>>()),
            ),
        };
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
        bodies.push(json!({"kind": kind, "content": body}));
    }
    let summary = counts
        .iter()
        .map(|(k, n)| format!("{k} ×{n}"))
        .collect::<Vec<_>>()
        .join(" · ");
    (summary, json!({"events": bodies}))
}

pub type SharedCommLog = std::sync::Arc<dyn ICommLog>;

#[cfg(test)]
#[path = "../../tests/unit/comm_log.rs"]
mod tests;
