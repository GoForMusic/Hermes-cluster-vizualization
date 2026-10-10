# Deploying Hermes

The hub is one container: the web app, the REST/SSE API for the browser and gRPC for the agents, all on port 8080 (plain HTTP/2, "h2c").
Put a TLS proxy in front of it. The database is SQLite, a file under `/data`: it needs a volume, and **one** hub instance only.

Two things must survive a redeploy: the `/data` volume and `HUB_SECRET_KEY` (64 hex characters). The key encrypts the credentials the
hub stores (kubeconfigs, agent tokens); without it they cannot be read, so back it up next to the database.

| | |
|---|---|
| `docker-compose/` | Hub + Traefik with Let's Encrypt, on one machine |
| `helm/hermes/` | The hub as a Kubernetes Deployment + PVC + Secret + Service (+ Ingress) |

## Docker Compose

```bash
cd deploy/docker-compose
./setup.sh                 # writes .env: DOMAIN, ACME_EMAIL and a generated HUB_SECRET_KEY
docker compose up -d
```

`DOMAIN` must point at the machine, with ports 80 and 443 open. `DOMAIN=localhost` works for a trial (Traefik then serves a self-signed certificate).
The hub itself publishes no port, only Traefik does. Traefik is told that the hub speaks h2c and that streams have no deadline
(`readtimeout=0`): agents keep one gRPC stream open for as long as they run.

### With Cloudflare

```bash
# .env: DOMAIN=hermes.example.com, ACME_EMAIL=..., CF_DNS_API_TOKEN=...
docker compose -f docker-compose.yml -f docker-compose.cloudflare.yml up -d
```

The certificate is requested with the DNS-01 challenge through the Cloudflare API, so port 80 does not have to be reachable and the proxy
(orange cloud) can be on. The token needs *Zone / DNS / Edit* and *Zone / Zone / Read* on the zone. In Cloudflare:
**SSL/TLS → Full (strict)**, and with the proxy on, enable **Network → gRPC**, or the agents cannot connect through it.

Upgrade: set `HERMES_VERSION` in `.env`, `docker compose pull && docker compose up -d`.
Back up the data: `docker run --rm -v hermes_hermes-data:/data -v "$PWD":/b busybox tar czf /b/hermes-data.tgz -C /data .`
(stop the hub first for a clean copy of the SQLite file: `docker compose stop hub`).

Traefik reads the Docker socket (read-only) to find the hub. That is the usual setup, but whoever owns Traefik can see every container;
put a socket proxy in front of it if the host runs other things.

## Kubernetes (Helm)

```bash
helm install hermes ./deploy/helm/hermes -n hermes --create-namespace \
  --set ingress.enabled=true --set ingress.host=hermes.example.com
```

- **Key:** with nothing set, the chart generates `HUB_SECRET_KEY` on first install and keeps it on upgrades (a Secret with
  `helm.sh/resource-policy: keep`). That needs a live cluster (`lookup`): with `helm template` or Argo CD, set `secret.existingSecret`
  or `secret.value`.
- **Data:** a PVC (`persistence.size`, `persistence.storageClass`), also kept on `helm uninstall`. The Deployment has one replica and the
  `Recreate` strategy, because SQLite has a single writer.
- **Ingress / h2c:** the defaults are for Traefik (the Service carries `traefik.ingress.kubernetes.io/service.serversscheme: h2c`).
  For ingress-nginx the web app and gRPC share a port, which its `backend-protocol: GRPC` would break for REST; use Traefik, or Gateway API
  with an `h2c` backend, or let the hub terminate TLS itself (`HUB_TLS_CERT`/`HUB_TLS_KEY`, mount the certificate and expose the port as is).
- The pod runs as 65532, read-only root filesystem, all capabilities dropped, no service-account token.
- **Upgrade:** `helm upgrade hermes ./deploy/helm/hermes -n hermes --set image.tag=1.0.2`.

## Agents

After the first login the web app asks where the agent images come from (default: the public images on `ghcr.io/goformusic`). Add a
source in **Admin → Sources** and run the install command it writes: the agent connects to `https://<your domain>` over gRPC.
