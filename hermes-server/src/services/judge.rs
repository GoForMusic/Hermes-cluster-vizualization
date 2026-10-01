//! The pure evaluation: given the live state, what alerts should exist and what should every node's heartbeat bucket say.
//! No side effects, no persistence — `Engine` (in `engine.rs`) is the only thing that acts on what this returns.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::rule::{self, Rule};
use crate::model::{Node, Severity, Source};

/// One time bucket of a node's heartbeat bar. Stored in SQLite and sent to the browser as these same lowercase strings (via
/// `as_str`) — the frontend uses the string directly as a CSS class name, so the four spellings cannot change without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Beat {
    Up,
    Warn,
    Down,
    NoData,
}

impl Beat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Beat::Up => "up",
            Beat::Warn => "warn",
            Beat::Down => "down",
            Beat::NoData => "nodata",
        }
    }
}

/// What the rules want to be true right now: one alert.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Want {
    pub(crate) sev: Severity,
    pub(crate) node_id: String,
    pub(crate) title: String,
    pub(crate) detail: String,
    /// What the node looked like right now, in case this `Want` is about to become a brand new alert (see `node_snapshot`).
    pub(crate) snapshot: String,
}

/// What a node looked like at this instant — its own status and metrics, and whatever its meta holds (image, restarts, containers,
/// …) — captured for a new alert's `snapshot`: the node itself may not exist any more by the time someone opens the incident.
fn node_snapshot(n: &Node) -> String {
    json!({ "own": n.own, "reason": n.reason, "m": n.m, "meta": n.meta }).to_string()
}

fn number(node: &Node, key: &str) -> f64 {
    node.meta
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
}

pub(crate) fn fmt_size(gib: f64) -> String {
    if gib >= 1024.0 {
        format!("{:.2} TiB", gib / 1024.0)
    } else if gib < 10.0 {
        format!("{gib:.1} GiB")
    } else {
        format!("{gib:.0} GiB")
    }
}

/// A control-plane node or a Swarm manager is not "a worker": the alert title says what kind of machine is down.
fn host_label(node: &Node) -> &'static str {
    match node.meta.get("role").and_then(|r| r.as_str()) {
        Some("control-plane") => "Control-plane node",
        Some("manager") => "Manager",
        _ => "Worker",
    }
}

fn or_default<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.is_empty() { default } else { s }
}

/// The name of the cluster a node belongs to, or nothing when its parents do not lead to one.
fn cluster_of<'a>(mut n: &'a Node, by: &HashMap<&str, &'a Node>) -> Option<&'a Node> {
    while n.kind != "cluster" {
        n = n.parent.as_deref().and_then(|p| by.get(p)).copied()?;
    }
    Some(n)
}

fn cluster_name<'a>(n: &'a Node, by: &HashMap<&str, &'a Node>) -> String {
    cluster_of(n, by).map_or(String::new(), |c| c.name.clone())
}

/// A volume's severity by usage, or none: the one ladder that decides both its heartbeat bucket and whether an alert fires for it, so
/// the two can no longer drift apart from each other.
fn volume_severity(used_pct: f64, size: f64, volume: Rule) -> Option<Severity> {
    if size <= 0.0 {
        return None;
    }
    if used_pct >= volume.crit {
        Some(Severity::Crit)
    } else if used_pct >= volume.value {
        Some(Severity::Warn)
    } else {
        None
    }
}

/// The alerts that should exist for this state of the world, and the heartbeat status of every node.
pub(crate) fn judge(
    nodes: &[Node],
    sources: &[Source],
    rules: &HashMap<String, Rule>,
) -> (HashMap<String, Want>, Vec<(String, Beat)>) {
    let by: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut kids: HashMap<&str, usize> = HashMap::new();
    for n in nodes {
        if let Some(p) = &n.parent {
            *kids.entry(p.as_str()).or_default() += 1;
        }
    }
    let rule_ = |id: &str| rules.get(id).copied().unwrap_or_default();
    let volume = rule_(rule::VOLUME_USAGE);
    // A host whose own heartbeat is fine but is running a struggling workload (a pod stuck pending, a crashing container) or a
    // nearly-full volume is not fully healthy either — its heartbeat bar should say so too, not just its live status. Capped at
    // "warn": one bad pod does not make the machine itself unreachable, and the workload's/volume's own alert already explains why.
    let mut child_trouble: HashSet<&str> = HashSet::new();
    for n in nodes {
        if n.stale {
            continue; // tells us nothing new about it
        }
        let troubled = match n.kind.as_str() {
            "workload" => n.own == "crit" || n.own == "warn",
            "volume" => {
                let size = number(n, "size");
                let used_pct = if size > 0.0 {
                    n.m.get("used").copied().unwrap_or(0.0) / size * 100.0
                } else {
                    0.0
                };
                volume_severity(used_pct, size, volume).is_some()
            }
            _ => false,
        };
        if troubled && let Some(p) = &n.parent {
            child_trouble.insert(p.as_str());
        }
    }
    // A source with its own topology collector still alive (Kubernetes' API, Swarm's manager) can know a specific host is down
    // (`own`) even while that host's own supplementary agent has gone stale — the two are different collectors. Only when the
    // source itself cannot be reached at all is nothing above `stale` still fresh enough to trust.
    let unreachable_sources: HashSet<&str> = sources
        .iter()
        .filter(|s| s.state == "error")
        .map(|s| s.id.as_str())
        .collect();

    let mut desired = HashMap::new();
    let mut beats = Vec::new();
    for n in nodes {
        let host = n.parent.as_deref().and_then(|p| by.get(p)).copied();
        let host_down =
            n.kind != "host" && host.is_some_and(|h| h.kind == "host" && h.own == "crit");
        let host_name = host.map_or("", |h| h.name.as_str());
        let size = number(n, "size");
        let used_pct = if size > 0.0 {
            n.m.get("used").copied().unwrap_or(0.0) / size * 100.0
        } else {
            0.0
        };

        // heartbeat status of this node
        let mut beat = match n.kind.as_str() {
            "host" if n.own == "crit" => Beat::Down,
            "host" if n.own == "warn" || child_trouble.contains(n.id.as_str()) => Beat::Warn,
            "host" => Beat::Up,
            "workload" if n.own == "crit" || host_down => Beat::Down,
            "workload" if n.own == "warn" => Beat::Warn,
            "workload" => Beat::Up,
            "volume" if host_down => Beat::Down,
            "volume" => match volume_severity(used_pct, size, volume) {
                Some(Severity::Crit) => Beat::Down,
                Some(Severity::Warn) => Beat::Warn,
                None => Beat::Up,
            },
            _ => continue,
        };
        // Stale is only "nothing is known" when there is no other, still-live collector vouching for this node: its own claim of
        // trouble (down_here) is either a fresh report from elsewhere, or the whole source is unreachable and stale by itself
        // does not make it any less so.
        let down_here = beat != Beat::Up;
        let source_down =
            cluster_of(n, &by).is_some_and(|c| unreachable_sources.contains(c.id.as_str()));
        let unreliable = n.stale && (!down_here || source_down);
        if unreliable {
            beat = Beat::NoData;
        }
        beats.push((n.id.clone(), beat));
        if unreliable {
            continue; // nothing fresh to alert on: either "up" and just quiet, or the source's own alert already says so
        }

        let cluster = cluster_name(n, &by);
        let mut want = |key: String, sev: Severity, title: String, detail: String| {
            desired.insert(
                key,
                Want {
                    sev,
                    node_id: n.id.clone(),
                    title,
                    detail,
                    snapshot: node_snapshot(n),
                },
            );
        };
        match n.kind.as_str() {
            "host" => {
                if n.own == "crit" && rule_(rule::HOST_DOWN).enabled {
                    let workloads = kids.get(n.id.as_str()).copied().unwrap_or(0);
                    want(
                        format!("host:{}", n.id),
                        Severity::Crit,
                        format!("{} {} unreachable", host_label(n), n.name),
                        format!(
                            "{cluster} · {} · {workloads} workloads affected",
                            or_default(&n.reason, "no heartbeat")
                        ),
                    );
                }
                let iac = n.meta.get("iac").and_then(|v| v.as_object());
                if rule_(rule::IAC_DRIFT).enabled
                    && iac
                        .and_then(|i| i.get("drift"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                {
                    let note = iac
                        .and_then(|i| i.get("note"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    want(
                        format!("drift:{}", n.id),
                        Severity::Warn,
                        format!("Terraform drift on {}", n.name),
                        note.to_string(),
                    );
                }
            }
            "workload" if n.own == "crit" && !host_down && rule_(rule::WORKLOAD_CRASH).enabled => {
                want(
                    format!("wl:{}", n.id),
                    Severity::Crit,
                    format!("{} {}", n.name, or_default(&n.reason, "failing")),
                    format!(
                        "{cluster} / {host_name} · restarts {:.0}",
                        number(n, "restarts")
                    ),
                );
            }
            "volume" if size > 0.0 && !host_down && volume.enabled => {
                if let Some(sev) = volume_severity(used_pct, size, volume) {
                    let used = n.m.get("used").copied().unwrap_or(0.0);
                    want(
                        format!("vol:{}", n.id),
                        sev,
                        format!("Volume {} at {used_pct:.0}%", n.name),
                        format!("{} of {} on {host_name}", fmt_size(used), fmt_size(size)),
                    );
                }
            }
            _ => {}
        }
    }

    // sources that cannot be reached
    for s in sources.iter().filter(|s| s.state == "error") {
        desired.insert(
            format!("src:{}", s.id),
            Want {
                sev: Severity::Crit,
                node_id: s.id.clone(),
                title: format!("Source {} unreachable", s.name),
                detail: s.info.clone(),
                snapshot: String::new(), // a whole source, not one node's state — nothing to snapshot
            },
        );
    }
    (desired, beats)
}
