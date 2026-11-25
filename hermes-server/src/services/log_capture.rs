//! The log of a container that is in trouble, kept with the alert. A pod that is deleted takes its log with it, and the reason it was
//! in trouble is in the last lines of that log: so when an alert opens for a workload, and the source lets the hub read logs, the last lines
//! are read once and written into the alert, where the incident report finds them whatever happens to the container after.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tracing::{debug, warn};

use crate::database::{IAlertDAO, now_ms};
use crate::model::Alert;
use crate::services::{IStore, LogEvent, LogOptions, LogServiceImp};

/// How many of the last lines are kept, and the most text.
const LINES: usize = 200;
const MAX_TEXT: usize = 64 * 1024;
/// A log that is not there within this long is not waited for.
const WITHIN: Duration = Duration::from_secs(15);
/// An alert that is older than this when the hub first sees it (the hub was restarted) is not captured: it is not what the log looked like.
const FRESH_MS: i64 = 2 * 60 * 1000;

/// Looks for new alerts on workloads every few seconds, until the process ends.
pub async fn run(alerts: Arc<dyn IAlertDAO>, store: Arc<dyn IStore>, logs: Arc<LogServiceImp>) {
    let mut tried: HashSet<i64> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(3));
    loop {
        tick.tick().await;
        let active = alerts.active_alerts().unwrap_or_default();
        tried.retain(|id| active.iter().any(|a| a.id == *id));
        for alert in active {
            if !alert.logs.is_empty()
                || alert.node_id.is_empty()
                || now_ms() - alert.ts > FRESH_MS
                || !tried.insert(alert.id)
            {
                continue;
            }
            let (alerts, store, logs) = (alerts.clone(), store.clone(), logs.clone());
            tokio::spawn(async move {
                let Some(text) = capture(&store, &logs, &alert).await else {
                    return;
                };
                if let Err(e) = alerts.set_alert_logs(alert.id, &text) {
                    warn!("log of alert {}: {e:#}", alert.id);
                    return;
                }
                // the alert was sent to the browsers when it opened, without the log: it is sent again, with it
                if let Ok(Some(with_log)) = alerts.get_alert(alert.id) {
                    store.publish(
                        &serde_json::json!({"type": "alert", "alert": with_log, "isNew": false}),
                    );
                }
            });
        }
    }
}

/// The log of the workload the alert is about, as text; `None` when there is nothing to read.
async fn capture(
    store: &Arc<dyn IStore>,
    logs: &Arc<LogServiceImp>,
    alert: &Alert,
) -> Option<String> {
    let node = store.nodes().into_iter().find(|n| n.id == alert.node_id)?;
    if node.kind != "workload" {
        return None;
    }
    let source = alert.node_id.split(':').next()?;
    if !logs.can_read(source) {
        return None;
    }
    let text = |key: &str| {
        node.meta
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let options = LogOptions {
        namespace: text("ns"),
        name: node.name.clone(),
        task: text("taskID"),
        tail: u32::try_from(LINES).unwrap_or(200),
        timestamps: true,
        ..LogOptions::default()
    };
    let restarted = node
        .meta
        .get("restarts")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        > 0.0;
    let mut out = String::new();
    if restarted
        && let Ok(lines) = read(
            logs,
            source,
            LogOptions {
                previous: true,
                ..options.clone()
            },
        )
        .await
        && !lines.is_empty()
    {
        out.push_str("── before the last restart ──\n");
        out.push_str(&lines.join("\n"));
        out.push('\n');
    }
    match read(logs, source, options).await {
        Ok(lines) if !lines.is_empty() => {
            if !out.is_empty() {
                out.push_str("── since ──\n");
            }
            out.push_str(&lines.join("\n"));
            out.push('\n');
        }
        Ok(_) => {}
        Err(why) => debug!("log of {}: {why}", alert.node_id),
    }
    if out.len() > MAX_TEXT {
        // the end is what matters: it is the last thing the container said
        let mut cut = out.len() - MAX_TEXT;
        while !out.is_char_boundary(cut) {
            cut += 1;
        }
        out = format!("… [earlier lines left out]\n{}", &out[cut..]);
    }
    (!out.is_empty()).then_some(out)
}

/// The last lines of a log that is not followed.
async fn read(
    logs: &Arc<LogServiceImp>,
    source: &str,
    options: LogOptions,
) -> Result<Vec<String>, String> {
    let mut stream = logs
        .open(source, &options)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let mut lines = Vec::new();
    let finished = tokio::time::timeout(WITHIN, async {
        loop {
            match stream.next().await {
                Some(LogEvent::Lines(mut l)) => {
                    lines.append(&mut l);
                    if lines.len() > LINES * 2 {
                        lines.drain(..lines.len() - LINES);
                    }
                }
                Some(LogEvent::End) | None => return Ok(()),
                Some(LogEvent::Failed(why)) => return Err(why),
            }
        }
    })
    .await;
    match finished {
        Ok(Ok(())) | Err(_) => {}
        Ok(Err(why)) => return Err(why),
    }
    let keep = lines.len().saturating_sub(LINES);
    Ok(lines.split_off(keep))
}
