//! Lets a node agent say which pods run on its own node without asking the Kubernetes API: it reads the container runtime
//! (containerd, CRI-O) through the CRI socket. That is what keeps the pods of a live node shown as running when the control
//! plane, the only other source of their state, is down.
//!
//! The socket gives full control of the node's containers, not read access only. The node agent uses it for one call, listing
//! running containers, and the install manifest says so.

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result};
use hermes_agentkit::SharedSink;
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;
use tracing::warn;

#[allow(clippy::enum_variant_names)] // the names are the runtime's, not ours
pub mod api {
    tonic::include_proto!("runtime.v1");
}

use api::runtime_service_client::RuntimeServiceClient;
use api::{ContainerFilter, ContainerState, ContainerStateValue, ListContainersRequest};

const EVERY: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(4);

// The labels the kubelet puts on every container it starts.
const LABEL_POD: &str = "io.kubernetes.pod.name";
const LABEL_NAMESPACE: &str = "io.kubernetes.pod.namespace";

/// Connects to the runtime's unix socket. Nothing is opened until the first call, so a socket that is not there yet is not a
/// startup failure: it shows up as an empty list of pods.
pub fn client(socket: &str) -> Result<RuntimeServiceClient<Channel>> {
    let path = socket.strip_prefix("unix://").unwrap_or(socket).to_string();
    // the URI is a placeholder: tonic wants one, the connector below decides where to go
    let endpoint = Endpoint::try_from("http://runtime.invalid").context("bad endpoint")?;
    let channel = endpoint.connect_with_connector_lazy(service_fn(move |_: Uri| {
        let path = path.clone();
        async move { Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(path).await?)) }
    }));
    Ok(RuntimeServiceClient::new(channel))
}

/// The hub ids of the pods that have a running container on this node, sorted, once each. The id is the one the cluster collector
/// gives a pod: `<source>:p:<namespace>:<name>`.
pub async fn pod_ids(
    rt: &mut RuntimeServiceClient<Channel>,
    source_id: &str,
) -> Result<Vec<String>> {
    let request = ListContainersRequest {
        filter: Some(ContainerFilter {
            state: Some(ContainerStateValue {
                state: ContainerState::ContainerRunning.into(),
            }),
            ..Default::default()
        }),
    };
    let response = tokio::time::timeout(CALL_TIMEOUT, rt.list_containers(request))
        .await
        .context("the runtime did not answer in time")??;

    let mut ids = BTreeSet::new();
    for container in response.into_inner().containers {
        let (Some(name), Some(namespace)) = (
            container.labels.get(LABEL_POD),
            container.labels.get(LABEL_NAMESPACE),
        ) else {
            continue; // not a Kubernetes pod (something started by hand on the node)
        };
        if !name.is_empty() && !namespace.is_empty() {
            ids.insert(format!("{source_id}:p:{namespace}:{name}"));
        }
    }
    Ok(ids.into_iter().collect())
}

/// Reports the pods running on this node every few seconds. When the runtime cannot be read it reports an empty list, so nothing
/// is vouched for that cannot be.
pub async fn run(source_id: &str, socket: &str, sink: SharedSink) -> Result<()> {
    let mut rt = client(socket).with_context(|| format!("container runtime socket {socket}"))?;
    let mut failing = false;
    loop {
        let ids = match pod_ids(&mut rt, source_id).await {
            Ok(ids) => {
                failing = false;
                ids
            }
            Err(e) => {
                if !failing {
                    warn!("node agent: cannot list containers on {socket}: {e:#}");
                }
                failing = true;
                Vec::new()
            }
        };
        sink.alive(ids);
        tokio::time::sleep(EVERY).await;
    }
}

#[cfg(test)]
#[path = "../tests/unit/cri.rs"]
mod tests;
