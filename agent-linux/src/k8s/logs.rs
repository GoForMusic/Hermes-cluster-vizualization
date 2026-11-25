//! Reading the log of a pod for the dashboard's log panel: the hub asks, and this reads it from the Kubernetes API and streams the lines
//! back. Only when the admin allowed it at install (`LOGS=1`, and the ClusterRole has `pods/log: get`); nothing is kept.

use std::time::Duration;

use futures_util::StreamExt;
use futures_util::io::AsyncBufReadExt;
use hermes_agentkit::logs::Batcher;
use hermes_agentkit::{ILogs, LogError, LogQuery};
use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, LogParams};
use tokio::sync::mpsc::Sender;

/// A followed log with nothing new for this long sends what it has: the panel shows a line when it is written, not when 500 are.
const FLUSH_AFTER: Duration = Duration::from_millis(200);

pub struct K8sLogs {
    client: Client,
}

impl K8sLogs {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

/// What the API said, in words for the person who asked.
fn explain(e: &kube::Error, what: &str) -> LogError {
    match e {
        kube::Error::Api(status) if status.code == 404 => LogError::NotHere(format!("{what} not found")),
        kube::Error::Api(status) if status.code == 403 => LogError::Failed(
            "this agent is not allowed to read pod logs: add the source again with \"Allow reading container logs\" ticked and apply that manifest once".into(),
        ),
        kube::Error::Api(status) if status.message.contains("previous terminated container") => LogError::Failed(
            "this container has not been restarted: there is no earlier log".into(),
        ),
        kube::Error::Api(status) => LogError::Failed(status.message.clone()),
        other => LogError::Failed(format!("cannot read the log: {other}")),
    }
}

#[tonic::async_trait]
impl ILogs for K8sLogs {
    async fn read(&self, q: LogQuery, out: Sender<Vec<String>>) -> Result<(), LogError> {
        let what = format!("pod {}/{}", q.namespace, q.name);
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &q.namespace);
        // the pod first: to say "not found" before anything, and to know its containers (the first one is the default)
        let pod = pods
            .get_opt(&q.name)
            .await
            .map_err(|e| explain(&e, &what))?
            .ok_or_else(|| LogError::NotHere(format!("{what} not found")))?;
        let spec = pod.spec.unwrap_or_default();
        let names: Vec<&str> = spec
            .containers
            .iter()
            .chain(spec.init_containers.iter().flatten())
            .map(|c| c.name.as_str())
            .collect();
        let container = if q.container.is_empty() {
            spec.containers
                .first()
                .map(|c| c.name.clone())
                .unwrap_or_default()
        } else if names.contains(&q.container.as_str()) {
            q.container.clone()
        } else {
            return Err(LogError::Failed(format!(
                "{what} has no container {:?} (it has: {})",
                q.container,
                names.join(", ")
            )));
        };
        let params = LogParams {
            container: (!container.is_empty()).then_some(container),
            follow: q.follow,
            previous: q.previous,
            tail_lines: Some(i64::from(q.tail)),
            since_seconds: (q.since_seconds > 0).then_some(i64::from(q.since_seconds)),
            timestamps: q.timestamps,
            ..LogParams::default()
        };
        let stream = pods
            .log_stream(&q.name, &params)
            .await
            .map_err(|e| explain(&e, &what))?;
        let mut lines = stream.lines();
        let mut batch = Batcher::new(out);
        loop {
            match tokio::time::timeout(FLUSH_AFTER, lines.next()).await {
                Ok(Some(Ok(line))) => {
                    if !batch.push(line).await {
                        return Ok(()); // the viewer left
                    }
                }
                Ok(Some(Err(e))) => {
                    batch.flush().await;
                    return Err(LogError::Failed(format!("the log stopped: {e}")));
                }
                Ok(None) => {
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

#[cfg(test)]
#[path = "../../tests/unit/k8s_logs.rs"]
mod tests;
