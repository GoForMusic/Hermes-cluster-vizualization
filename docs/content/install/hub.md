# Install the hub

The hub is one container: the web app, the REST/SSE API for the browser and gRPC for the agents, all on **one port** (plain HTTP/2, "h2c"). Put a TLS proxy in front of
it, or let the hub terminate TLS itself. The database is SQLite, a file under `/data`: it needs a volume, and **one** hub instance only.

!!! warning "Two things must survive a redeploy"
    The `/data` volume and `HUB_SECRET_KEY` (64 hex characters). The key encrypts what the hub stores: agent tokens, the registry password, identity providers' secrets.
    Without it they cannot be read. Back it up next to the database.

## Docker Compose with Traefik and Let's Encrypt

```bash
cd deploy/docker-compose
./setup.sh                 # writes .env: DOMAIN, ACME_EMAIL and a generated HUB_SECRET_KEY
docker compose up -d
```

`DOMAIN` must point at the machine, with ports 80 and 443 open (`DOMAIN=localhost` works for a trial: Traefik then serves a self-signed certificate).
The hub publishes no port, only Traefik does. Traefik is told that the hub speaks h2c and that streams have no deadline (`readtimeout=0`): agents keep one gRPC stream open for as long as they run.

=== "With Cloudflare"

    ```bash
    # .env: DOMAIN=hermes.example.com, ACME_EMAIL=..., CF_DNS_API_TOKEN=...
    docker compose -f docker-compose.yml -f docker-compose.cloudflare.yml up -d
    ```

    The certificate is requested with the DNS-01 challenge through the Cloudflare API, so port 80 does not have to be reachable and the proxy (orange cloud) can be on. The token needs
    *Zone / DNS / Edit* and *Zone / Zone / Read*. In Cloudflare set **SSL/TLS → Full (strict)** and, with the proxy on, enable **Network → gRPC**, or the agents cannot connect through it.

**Upgrade:** set `HERMES_VERSION` in `.env`, then `docker compose pull && docker compose up -d`.
**Back up:** stop the hub for a clean copy of the SQLite file, then `docker run --rm -v hermes_hermes-data:/data -v "$PWD":/b busybox tar czf /b/hermes-data.tgz -C /data .`

## Kubernetes (Helm)

```bash
helm install hermes ./deploy/helm/hermes -n hermes --create-namespace \
  --set ingress.enabled=true --set ingress.host=hermes.example.com
```

| Setting | |
|---|---|
| `secret.existingSecret`, `secret.value` | the key. With neither, the chart generates one on first install and keeps it on upgrades (needs a live cluster for `lookup`; with `helm template` or Argo CD set one of the two) |
| `persistence.size`, `.storageClass`, `.existingClaim` | the SQLite volume, kept on `helm uninstall`. One replica, `Recreate` strategy |
| `ingress.enabled`, `.host`, `.className`, `.tls.*` | the Ingress. Defaults are for Traefik (the Service is annotated `…/service.serversscheme: h2c`) |
| `hub.basePath`, `hub.publicUrl` | [a sub-path](#behind-a-proxy-on-a-sub-path) and the public address ([SSO](../security/sso.md)) |
| `hub.agentImage`, `.agentImageWindows`, `.agentPullSecret` | the images the generated manifests point at |

The pod runs as 65532 with a read-only root filesystem, all capabilities dropped and no service-account token.
**Upgrade:** `helm upgrade hermes ./deploy/helm/hermes -n hermes --set image.tag=1.1.0`.

!!! note "Ingress controllers"
    The web app and gRPC share a port. ingress-nginx's `backend-protocol: GRPC` would break the REST side: use Traefik, or Gateway API with an `h2c` backend,
    or let the hub terminate TLS itself.

## TLS in the hub

Skip the proxy: give the hub a certificate and key (PEM) and it serves HTTPS itself, with the same one port.

```yaml
environment:
  HUB_TLS_CERT: /certs/cert.pem
  HUB_TLS_KEY: /certs/key.pem        # both, or neither
volumes:
  - ./certs:/certs:ro
```

Agents then use `https://` in `HUB_URL`; a private CA goes in the agents' `HUB_CA_FILE`.

## Behind a proxy on a sub-path

Traefik routes `host/hermes` to the container and does **not** strip the prefix? Set `HUB_BASE_PATH=/hermes`. The hub serves the web app, the API, the event stream and gRPC under it, and
the session cookie belongs to that path only. Everything outside it is a 404.

```yaml
environment:
  HUB_BASE_PATH: /hermes
labels:
  - traefik.http.routers.hermes.rule=PathPrefix(`/hermes`)
```

The agents are given the path in their address: `HUB_URL=https://host/hermes`. Health checks are at `/hermes/api/health`.
Set `HUB_PUBLIC_URL=https://host` when the hub cannot tell its own public address from the request (it is what the [SSO](../security/sso.md) callback address is built from).

## Federation

When a firewall lets the main hub reach a zone but not the other way round, install a second hub there and let the main hub manage it: [Federation](federation.md).
