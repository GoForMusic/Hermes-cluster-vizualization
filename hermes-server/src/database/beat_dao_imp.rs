//! Heartbeats: one row per node per status change, replayed into Kuma-style uptime bars.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rusqlite::params;

use crate::database::IDbContext;
use crate::model::Uptime;

pub trait IBeatDAO: Send + Sync {
    fn insert_beat(&self, node_id: &str, ts: i64, status: &str) -> Result<()>;
    /// Replays the status changes over the last `span` and returns Kuma-style bars for every node.
    fn uptime(&self, now_ms: i64, span_ms: i64, buckets: usize) -> Result<HashMap<String, Uptime>>;
}

pub struct BeatDAOImp {
    ctx: Arc<dyn IDbContext>,
}

impl BeatDAOImp {
    pub fn new(ctx: Arc<dyn IDbContext>) -> Self {
        Self { ctx }
    }
}

impl IBeatDAO for BeatDAOImp {
    fn insert_beat(&self, node_id: &str, ts: i64, status: &str) -> Result<()> {
        self.ctx.conn().execute(
            "INSERT INTO beats(node_id,ts,status) VALUES(?1,?2,?3)",
            params![node_id, ts, status],
        )?;
        Ok(())
    }

    fn uptime(&self, now_ms: i64, span_ms: i64, buckets: usize) -> Result<HashMap<String, Uptime>> {
        let (start, end) = (now_ms - span_ms, now_ms);
        let mut events: HashMap<String, Vec<(i64, String)>> = HashMap::new();
        let mut firsts: HashMap<String, i64> = HashMap::new();
        {
            let conn = self.ctx.conn();
            // what each node was doing when the window opened
            let mut prior = conn.prepare("SELECT node_id, status FROM beats b WHERE ts < ?1 AND ts = (SELECT MAX(ts) FROM beats WHERE node_id=b.node_id AND ts < ?1)")?;
            for row in prior.query_map([start], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (id, status) = row?;
                events.entry(id).or_default().push((start, status));
            }
            let mut inside = conn.prepare(
                "SELECT node_id, ts, status FROM beats WHERE ts >= ?1 ORDER BY node_id, ts",
            )?;
            for row in inside.query_map([start], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (id, ts, status) = row?;
                events.entry(id).or_default().push((ts, status));
            }
            let mut first = conn.prepare("SELECT node_id, MIN(ts) FROM beats GROUP BY node_id")?;
            for row in first.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
                let (id, ts) = row?;
                firsts.insert(id, ts);
            }
        }

        let bucket_ms = (end - start) / i64::try_from(buckets).unwrap_or(1).max(1);
        Ok(events
            .into_iter()
            .map(|(id, ev)| {
                let up = bars(&ev, start, bucket_ms, buckets);
                let current = ev.last().map(|e| e.1.clone()).unwrap_or_default();
                let first = firsts.get(&id).copied().unwrap_or_default();
                (
                    id,
                    Uptime {
                        bars: up.0,
                        pct: up.1,
                        current,
                        first,
                    },
                )
            })
            .collect())
    }
}

fn severity(status: &str) -> u8 {
    match status {
        "up" => 1,
        "warn" => 2,
        "down" => 3,
        _ => 0, // nodata
    }
}

/// The bar of each bucket is the worst thing that happened in it, or what the node was doing when it began. Returns the bars and the
/// share of observed buckets that were not down.
fn bars(
    events: &[(i64, String)],
    start: i64,
    bucket_ms: i64,
    buckets: usize,
) -> (Vec<String>, f64) {
    let (mut observed, mut down) = (0u32, 0u32);
    let mut out = Vec::with_capacity(buckets);
    for b in 0..buckets {
        let b = i64::try_from(b).unwrap_or(0);
        let (t0, t1) = (start + b * bucket_ms, start + (b + 1) * bucket_ms);
        let mut worst = "nodata".to_string();
        let mut have = false;
        if let Some((_, status)) = events.iter().rfind(|(ts, _)| *ts <= t0) {
            worst.clone_from(status);
            have = true;
        }
        for (ts, status) in events {
            if *ts > t0 && *ts < t1 {
                have = true;
                if severity(status) > severity(&worst) {
                    worst.clone_from(status);
                }
            }
        }
        if have {
            observed += 1;
            if worst == "down" {
                down += 1;
            }
        }
        out.push(worst);
    }
    let pct = if observed > 0 {
        100.0 * f64::from(observed - down) / f64::from(observed)
    } else {
        100.0
    };
    (out, pct)
}
