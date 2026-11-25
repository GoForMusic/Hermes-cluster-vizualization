# Configuration

Everything is an environment variable.

## The hub

| Variable | Default | |
|---|---|---|
| `HUB_ADDR` | `:8080` | listen address, for the web app, the API and gRPC (one port) |
| `HUB_DATA` | `/data` | the SQLite database lives here: a volume |
| `HUB_WEB` | `/web` | the web app's files |
| `HUB_SECRET_KEY` | **required** | 64 hex characters (`openssl rand -hex 32`). Encrypts the stored secrets. Keep it |
| `HUB_BASE_PATH` | `/` | serve everything under a prefix: [behind a proxy](../install/hub.md#behind-a-proxy-on-a-sub-path) |
| `HUB_PUBLIC_URL` | from the request | the hub's public address, for the SSO callback |
| `HUB_TLS_CERT`, `HUB_TLS_KEY` | off | PEM files: the hub serves HTTPS itself (both or neither) |
| `HUB_AGENT_IMAGE` | | the Linux agent image the manifests point at |
| `HUB_AGENT_IMAGE_WINDOWS` | | the Windows agent image (adds the Windows service to Swarm manifests) |
| `HUB_AGENT_PULL_SECRET` | | name of the registry pull secret created in the cluster |
| `HUB_FEDERATION` | on | `off` undoes *Managed by another hub* by hand |
| `HUB_FEDERATION_CA_FILE` | | a CA for the link to managed hubs |
| `HUB_DEMO` | `0` | `1` loads a fixture for development; keep it off |

## The agents

| Variable | |
|---|---|
| `HUB_URL` | **required.** The hub's address, with its base path: `https://host/hermes` |
| `SOURCE_ID`, `SOURCE_NAME`, `TOKEN` | **required.** Written into the manifest |
| `AGENT_ID`, `AGENT_HOST` | the agent's name and the node it runs on (defaults from the machine) |
| `HUB_CA_FILE` | a private CA for the hub's certificate |
| `COLLECTOR` | `kubernetes` (default) or `node` on the Linux agent (set by the manifest) |
| `CRI_SOCKET` | the node agent's container runtime socket: it then says which pods run on the node |
| `FLOWS` | `1`: read the kernel's connection table to show who talks to whom (Kubernetes) |
| `UPGRADES` | `1`: the agent may change its own image |
| `LOGS` | `1`: the agent may read container logs |
| `DOCKER_SOCKET` | the Docker engine's socket (Swarm and Docker agents) |
| `NODE_LOCATION` | a label for where the machine is (`rack-2`) |

## Ports

One: the hub's. Agents connect **out** to it, over `https`, with HTTP/2 (gRPC). A proxy in front must allow HTTP/2 to the hub (h2c, or TLS) and streams that last for ever.
