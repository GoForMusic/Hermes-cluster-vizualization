//! Raw Docker/Swarm state strings turned into what they mean: a node's availability, a task's lifecycle, and whether a failure is
//! still recent enough to count as "now". Mirrors `agent-linux`'s `k8s::state` — same reasoning, same shape, the other collector.
//!
//! Swarm keeps the last few tasks of every slot, so a failure can be old news: a service that was updated an hour ago, or a node that
//! rebooted, leaves failed tasks behind. Only recent failures say a service is failing now.

use hermes_proto::v1::Own;

use crate::docker::{SwarmNode, SwarmTask};

/// A failure older than this says nothing about now.
const FAILURE_WINDOW_MS: i64 = 10 * 60 * 1000;
/// How many recent failures make a service a crash loop. One or two are a restart worth a look (a node rebooted, a rolling update raced with
/// the engine), not an outage.
const CRASH_LOOP_AFTER: u32 = 3;

/// The first of these that isn't empty.
pub(crate) fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
    values
        .iter()
        .copied()
        .find(|v| !v.is_empty())
        .unwrap_or_default()
}

fn timestamp_ms(text: &str) -> Option<i64> {
    text.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

/// Did this failed task fail within the window? A task with no readable time counts: better a false alarm than a hidden crash loop.
pub fn failed_recently(task: &SwarmTask, now_ms: i64) -> bool {
    for text in [&task.status.timestamp, &task.updated_at] {
        if let Some(at) = timestamp_ms(text) {
            return now_ms - at < FAILURE_WINDOW_MS;
        }
    }
    true
}

/// A node's `spec.availability`. Not named `NodeStatus` — that's already `docker::NodeStatus`, the raw JSON shape of `status`;
/// this is what the three availability strings mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Availability {
    Active,
    Pause,
    Drain,
}

impl Availability {
    fn parse(s: &str) -> Self {
        match s {
            "drain" => Self::Drain,
            "pause" => Self::Pause,
            _ => Self::Active,
        }
    }
}

/// A task's `status.state`. `Other` covers the transient ones (`new`, `pending`, `assigned`, `preparing`, `starting`, ...), shown as
/// a plain warning with Docker's own word for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskState {
    Running,
    Failed,
    Rejected,
    Orphaned,
    Other,
}

impl TaskState {
    pub(crate) fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "failed" => Self::Failed,
            "rejected" => Self::Rejected,
            "orphaned" => Self::Orphaned,
            _ => Self::Other,
        }
    }
}

pub fn node_state(n: &SwarmNode) -> (Own, String) {
    if n.status.state != "ready" {
        return (Own::Crit, "NotReady".into());
    }
    if n.manager_status
        .as_ref()
        .is_some_and(|m| m.reachability != "reachable")
    {
        return (Own::Crit, "Unreachable manager".into());
    }
    match Availability::parse(&n.spec.availability) {
        Availability::Drain => (Own::Warn, "Drain".into()),
        Availability::Pause => (Own::Warn, "Paused".into()),
        Availability::Active => (Own::Ok, String::new()),
    }
}

/// Running is fine; a task that keeps failing is a crash loop; one failure or a plain start is a warning.
pub fn task_state(t: &SwarmTask, failed_before: u32, last_err: &str) -> (Own, String) {
    match TaskState::parse(&t.status.state) {
        TaskState::Running => return (Own::Ok, String::new()),
        TaskState::Failed | TaskState::Rejected | TaskState::Orphaned => {
            return (
                Own::Crit,
                first_non_empty(&[&t.status.err, &t.status.message, &t.status.state]).to_string(),
            );
        }
        TaskState::Other => {}
    }
    if failed_before >= CRASH_LOOP_AFTER {
        return (
            Own::Crit,
            format!("CrashLoop: {}", first_non_empty(&[last_err, "restarting"])),
        );
    }
    if failed_before >= 1 {
        return (
            Own::Warn,
            format!(
                "Restarting: {}",
                first_non_empty(&[last_err, "after a failure"])
            ),
        );
    }
    let mut chars = t.status.state.chars();
    let word = chars.next().map_or_else(
        || "Unknown".to_string(),
        |c| c.to_uppercase().chain(chars).collect(),
    );
    (Own::Warn, word)
}
