//! Who talks to whom on this node, from the kernel's connection tracking (`/proc/net/nf_conntrack`): for every connection the address that
//! opened it, the address it asked for, the address that answered (the pod behind a Service) and the bytes in each direction. The agent turns
//! the counters into rates and reports them per pair of addresses; the hub knows what the addresses are.
//!
//! What it needs from the node: the host network namespace (that is where the connections of every pod are tracked), a user that may read the
//! file (root: no capability at all), and `net.netfilter.nf_conntrack_acct=1`, which is what makes the kernel count bytes. It changes nothing
//! on the node: it only reads.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use hermes_agentkit::SharedSink;
use hermes_proto::v1::{CollectorState, Flow};
use tracing::warn;

const EVERY: Duration = Duration::from_secs(5);
const DEFAULT_FILE: &str = "/proc/net/nf_conntrack";

/// One tracked connection, as the kernel describes it: the original direction, and the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conn {
    pub proto: String,
    pub src: String,
    pub dst: String,
    pub sport: u16,
    pub dport: u16,
    pub bytes_out: u64,
    /// Who answered: the same as `dst` unless the connection was translated (a Service or a node port).
    pub reply_src: String,
    pub bytes_in: u64,
    /// The kernel counted bytes for it (`nf_conntrack_acct`).
    pub counted: bool,
}

/// Every TCP and UDP connection in the text of `/proc/net/nf_conntrack`. Lines that are not of that shape are left out.
pub fn parse(text: &str) -> Vec<Conn> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Conn> {
    let mut words = line.split_whitespace();
    let (_family, _, proto) = (words.next()?, words.next()?, words.next()?);
    if proto != "tcp" && proto != "udp" {
        return None;
    }
    let mut c = Conn {
        proto: proto.to_string(),
        src: String::new(),
        dst: String::new(),
        sport: 0,
        dport: 0,
        bytes_out: 0,
        reply_src: String::new(),
        bytes_in: 0,
        counted: false,
    };
    let mut group = 0; // the first `src=` starts the original direction, the second the reply
    for word in words {
        let Some((key, value)) = word.split_once('=') else {
            continue; // a state (TIME_WAIT), a flag ([ASSURED]) or the protocol number and timeout
        };
        match (key, group) {
            ("src", _) => {
                group += 1;
                if group == 1 {
                    c.src = value.to_string();
                } else {
                    c.reply_src = value.to_string();
                }
            }
            ("dst", 1) => c.dst = value.to_string(),
            ("sport", 1) => c.sport = value.parse().ok()?,
            ("dport", 1) => c.dport = value.parse().ok()?,
            ("bytes", 1) => {
                c.bytes_out = value.parse().ok()?;
                c.counted = true;
            }
            ("bytes", 2) => c.bytes_in = value.parse().ok()?,
            _ => {}
        }
    }
    (group == 2 && !c.src.is_empty() && !c.dst.is_empty()).then_some(c)
}

type Key = (String, String, u16, String, u16);
type Pair = (String, String, String, u16, String);

/// Turns the byte counters of the connections into rates. A counter says how much a connection has moved since it began, so what matters is
/// how much it grew since the last look; a connection first seen after the first look is new, and all it moved counts.
#[derive(Default)]
pub struct Tracker {
    seen: HashMap<Key, (u64, u64)>,
    primed: bool,
}

fn local(ip: &str) -> bool {
    ip.starts_with("127.") || ip == "::1"
}

impl Tracker {
    /// The flows over the last `seconds`, one for each pair of addresses and port, in Mbit/s.
    pub fn step(&mut self, conns: &[Conn], seconds: f64) -> Vec<Flow> {
        let mut now: HashMap<Key, (u64, u64)> = HashMap::new();
        let mut sums: HashMap<Pair, (u64, u64)> = HashMap::new();
        for c in conns
            .iter()
            .filter(|c| c.counted && !local(&c.src) && !local(&c.dst) && c.src != c.dst)
        {
            let key: Key = (
                c.proto.clone(),
                c.src.clone(),
                c.sport,
                c.dst.clone(),
                c.dport,
            );
            let grown = |current: u64, before: u64| {
                if current >= before {
                    current - before
                } else {
                    current
                }
            }; // a reused entry starts again
            let (out, back) = match self.seen.get(&key) {
                Some(&(o, i)) => (grown(c.bytes_out, o), grown(c.bytes_in, i)),
                None if self.primed => (c.bytes_out, c.bytes_in),
                None => (0, 0), // it was there before we looked: how much it had moved already is not this interval's
            };
            now.insert(key, (c.bytes_out, c.bytes_in));
            if out + back == 0 {
                continue;
            }
            let served_by = if c.reply_src == c.dst {
                String::new()
            } else {
                c.reply_src.clone()
            };
            let pair: Pair = (
                c.src.clone(),
                c.dst.clone(),
                served_by,
                c.dport,
                c.proto.clone(),
            );
            let sum = sums.entry(pair).or_default();
            sum.0 += out;
            sum.1 += back;
        }
        self.seen = now;
        self.primed = true;
        let mbps = |bytes: u64| bytes as f64 * 8.0 / 1e6 / seconds.max(0.001);
        let mut flows: Vec<Flow> = sums
            .into_iter()
            .map(|((src, dst, served_by, port, proto), (out, back))| Flow {
                src,
                dst,
                served_by,
                port: u32::from(port),
                proto,
                out_mbps: mbps(out),
                in_mbps: mbps(back),
            })
            .collect();
        flows.sort_by(|a, b| (&a.src, &a.dst, a.port).cmp(&(&b.src, &b.dst, b.port)));
        flows
    }
}

/// Reads the connection table every few seconds and reports the flows, until the file cannot be read (the caller restarts the collector).
pub async fn run(sink: SharedSink) -> Result<()> {
    let path = std::env::var("CONNTRACK_FILE")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| DEFAULT_FILE.to_string());
    let mut tracker = Tracker::default();
    let mut last = Instant::now();
    let mut warned = false;
    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let text = tokio::fs::read_to_string(&path).await.with_context(|| {
            format!("cannot read {path}: the agent needs the host network (hostNetwork), to run as root, and the nf_conntrack module")
        })?;
        let conns = parse(&text);
        let counted = conns.iter().any(|c| c.counted);
        if !counted && !conns.is_empty() && !warned {
            warned = true;
            warn!(
                "the kernel does not count bytes per connection: set net.netfilter.nf_conntrack_acct=1 on the node"
            );
        }
        let now = Instant::now();
        let flows = tracker.step(&conns, now.saturating_duration_since(last).as_secs_f64());
        last = now;
        sink.report(
            CollectorState::Connected,
            &format!(
                "flows: {} connections · byte counting {}",
                conns.len(),
                if counted || conns.is_empty() {
                    "on"
                } else {
                    "off (net.netfilter.nf_conntrack_acct=1 is needed)"
                }
            ),
        );
        sink.flows(flows);
    }
}

#[cfg(test)]
#[path = "../tests/unit/flows.rs"]
mod tests;
