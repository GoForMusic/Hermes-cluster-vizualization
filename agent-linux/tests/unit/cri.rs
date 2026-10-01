use std::collections::HashMap;
use std::sync::Arc;

use api::runtime_service_server::{RuntimeService, RuntimeServiceServer};
use api::{Container, ListContainersResponse};
use hermes_agentkit::testing::{Call, RecordingSink};
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::{Request, Response, Status};

use super::*;

/// Stands in for containerd. It refuses a request that does not ask for running containers only.
struct FakeRuntime;

fn container(labels: &[(&str, &str)]) -> Container {
    Container {
        labels: labels
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect::<HashMap<_, _>>(),
        ..Default::default()
    }
}

#[tonic::async_trait]
impl RuntimeService for FakeRuntime {
    async fn list_containers(
        &self,
        req: Request<ListContainersRequest>,
    ) -> Result<Response<ListContainersResponse>, Status> {
        let wanted = req
            .into_inner()
            .filter
            .and_then(|f| f.state)
            .map(|s| s.state);
        if wanted != Some(ContainerState::ContainerRunning.into()) {
            return Err(Status::invalid_argument(
                "the agent must ask for running containers only",
            ));
        }
        let pod = |ns, name| container(&[(LABEL_NAMESPACE, ns), (LABEL_POD, name)]);
        Ok(Response::new(ListContainersResponse {
            containers: vec![
                pod("kube-system", "coredns-1"),
                pod("default", "web-1"),
                pod("default", "web-1"), // a second container of the same pod
                container(&[]),          // started by hand: no pod labels
                container(&[(LABEL_POD, "half-labelled")]),
            ],
        }))
    }
}

fn serve(dir: &tempfile::TempDir) -> String {
    let path = dir.path().join("containerd.sock");
    let listener = UnixListener::bind(&path).unwrap();
    tokio::spawn(
        tonic::transport::Server::builder()
            .add_service(RuntimeServiceServer::new(FakeRuntime))
            .serve_with_incoming(UnixListenerStream::new(listener)),
    );
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn lists_each_running_pod_once_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let socket = serve(&dir);
    let mut rt = client(&format!("unix://{socket}")).unwrap();
    let ids = pod_ids(&mut rt, "src").await.unwrap();
    assert_eq!(ids, ["src:p:default:web-1", "src:p:kube-system:coredns-1"]);
}

#[tokio::test(start_paused = true)]
async fn reports_an_empty_list_when_the_runtime_cannot_be_read() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(RecordingSink::default());
    let socket = dir
        .path()
        .join("missing.sock")
        .to_string_lossy()
        .into_owned(); // nothing listens here
    let task = tokio::spawn({
        let sink: SharedSink = sink.clone();
        async move { run("src", &socket, sink).await }
    });
    tokio::time::sleep(Duration::from_secs(1)).await;
    task.abort();
    assert_eq!(sink.calls().first(), Some(&Call::Alive(vec![])));
}
