# agent-linux

The [Infra Viz](../../README.md) agent for Linux: one static Rust binary that reports to the hub over gRPC.

| `COLLECTOR` | Runs | Watches | State |
|---|---|---|---|
| `node` | one per node (a DaemonSet) | that this node is alive; with `CRI_SOCKET`, which pods run on it | works |
| `kubernetes` (default) | as one pod in a cluster | the whole cluster through the API: nodes, pods, volume claims, CPU and memory (metrics-server), volume usage and pod traffic (kubelet). Tested against a fake API server, **not yet against a real cluster** | works |
| `swarm` | as a global service, one per node | its own node (CPU, memory, the containers running on it); on a manager also the swarm topology (nodes, services, tasks). Tested against a fake Docker engine, **not yet against a real swarm** | works |

It only makes outbound connections to the hub. You normally do not configure it by hand: the hub generates the
install manifest (Admin → Sources → Add source) with the settings filled in.

## Settings (environment)

| Variable | |
|---|---|
| `HUB_URL` | where the hub is reachable from here, e.g. `https://hub.example.com` (`http://` = no TLS) |
| `SOURCE_ID`, `SOURCE_NAME`, `TOKEN` | issued by the hub for this source |
| `COLLECTOR` | `kubernetes` (default), `swarm` or `node` |
| `AGENT_ID` | this instance; defaults to the host name |
| `AGENT_HOST` | the host node this agent runs on; the manifest sets it |
| `CRI_SOCKET` | node only: the container runtime socket |
| `DOCKER_SOCKET` | swarm only: the Docker engine's socket, default `/var/run/docker.sock` |
| `HUB_CA_FILE` | optional: PEM file with the CA that signed the hub's certificate (a private CA) |
| `RUST_LOG` | log level, default `info` |

## Build

From the **repository root**:

```bash
cargo test -p hermes-agent-linux
docker build -f agent-linux/Dockerfile -t hermes-agent-linux:dev .     # a static binary in an empty image, about 10 MB
docker buildx build -f agent-linux/Dockerfile --platform linux/amd64 -t REGISTRY/hermes-agent-linux:1.0.0 --push .
```

Published by the pipeline when you push a tag `agent-linux-X.Y.Z` (see the main README). Runs as an unprivileged user.

## Layout

```
src/main.rs    picks the collector from the environment
src/linuxhost.rs  CPU and memory from /proc
src/cri.rs     the container runtime interface (containerd, CRI-O): lists running containers over the runtime's unix socket
src/k8s/       the Kubernetes collector: topology (pure: objects in, hub nodes out), state (ready/crash/pressure), quantity (100m, 2Gi), metrics (metrics-server),
               kubelet (volume usage, pod traffic), mod (the polling loop, over `kube`)
build.rs       generates the CRI client from proto/cri/v1/runtime.proto
```
