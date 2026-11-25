# API reference

Everything the web app does goes through this API, so anything you can click you can script.

## Conventions

- **Base:** `https://<hub>[/<HUB_BASE_PATH>]/api/…`. Bodies are JSON (`camelCase`).
- **Login:** `POST /api/auth/login` sets the cookie `hermes_session` (`HttpOnly`, `SameSite=Strict`, scoped to the base path).
- **CSRF:** every request that is not a `GET` must carry `X-Requested-With: hermes`, or it is refused with 403.
- **Authorization:** each call is judged by the caller's [policies](../security/policies.md): the method gives the capability (`GET` read, `POST` create, `PUT`/`PATCH` update, `DELETE` delete) and the URL without `/api/` gives the path. The *Policy path* column below is that path.
- **Errors:** `{"error": "…"}` with 400 (bad input), 401 (login needed), 403 (not allowed, or no CSRF header), 404, 409 (exists).
- **Seeing a cluster** (`clusters/<id>`) filters what the *live* endpoints return (marked ◆), not just whether they answer.

```bash
curl -c jar -H 'X-Requested-With: hermes' -H 'content-type: application/json' \
  -d '{"username":"admin","password":"…"}' https://hermes.example.com/api/auth/login
curl -b jar https://hermes.example.com/api/sources
curl -b jar -X POST -H 'X-Requested-With: hermes' https://hermes.example.com/api/alerts/12/ack
```

## Session and setup

| Call | Policy path | |
|---|---|---|
| `GET /api/health` | none | liveness, for proxies and orchestrators |
| `GET /api/auth/status` | none | is there an admin yet, am I logged in, which providers, is the wallboard public |
| `POST /api/auth/setup` | none | create the first admin (only while there are no users) |
| `POST /api/auth/login` | none | `{username, password}` → session cookie |
| `POST /api/auth/logout` | login | ends the session |
| `POST /api/auth/password` | login | change **my** password |
| `GET /api/auth/sso/{id}/login` | none | start a sign-in at provider `{id}` (redirects) |
| `GET /api/auth/sso/{id}/callback` | none | where the provider comes back |
| `PUT /api/auth/public-view` | `settings` | allow or forbid the wallboard without login |

## Live state

| Call | Policy path | |
|---|---|---|
| `GET /api/info` | wallboard or login | hub version, name, mode |
| `GET /api/snapshot` ◆ | wallboard or login | the whole map now |
| `GET /api/stream` ◆ | wallboard or login | Server-Sent Events: the snapshot, then changes. Ends when the session ends. |
| `GET /api/alerts` ◆ | wallboard or login | alerts, newest first (`?limit=` 1–1000, default 300) |
| `GET /api/uptime` ◆ | wallboard or login | uptime per node (`?span=` seconds, `?buckets=` 1–200) |
| `POST /api/alerts/{id}/ack` | `alerts/{id}/ack` | acknowledge |
| `GET /api/logs/{source}/{namespace}/{name}` | `logs/{source}/{namespace}/{name}` | a container's log as Server-Sent Events: [Logs](../using/logs.md) |

`?view=tv` on the live calls asks for the wallboard's view (every cluster, if the caller may read `wallboard`).

## Sources

| Call | Policy path | |
|---|---|---|
| `GET /api/sources` | login | the sources the caller sees; the connection details only with `sources` |
| `POST /api/sources` | `sources` | add one; the answer holds the manifest and the token (shown once) |
| `PATCH /api/sources/{id}` | `sources/{id}` | rename, change what it may do |
| `DELETE /api/sources/{id}` | `sources/{id}` | remove it |
| `POST /api/sources/{id}/upgrade` | `sources/{id}/upgrade` | `{version}`: ask its agents to change version |
| `GET /api/settings`, `PUT /api/settings` | `settings` | wallboard, name… |
| `GET /api/registry`, `PUT /api/registry` | `registry` | where the agent images are |
| `POST /api/registry/test`, `GET /api/registry/versions` | `registry/…` | test the login, list the versions |
| `GET /api/comm-log`, `PUT /api/comm-log`, `GET /api/comm-log/{id}` | `comm-log` | the hub ↔ agent communication log |

## Access

| Call | Policy path | |
|---|---|---|
| `GET /api/users`, `POST /api/users` | `users` | list, create |
| `PATCH /api/users/{id}`, `DELETE /api/users/{id}` | `users/{id}` | edit, turn off, delete |
| `GET /api/groups`, `POST /api/groups` | `groups` | |
| `PATCH /api/groups/{id}`, `DELETE /api/groups/{id}` | `groups/{id}` | |
| `PUT /api/groups/{id}/members` | `groups/{id}/members` | replace the members |
| `PUT /api/groups/{id}/policies` | `groups/{id}/policies` | replace the policies |
| `GET /api/policies`, `POST /api/policies` | `policies` | |
| `PATCH /api/policies/{id}`, `DELETE /api/policies/{id}` | `policies/{id}` | |
| `GET /api/policies/check` | `policies` | `?user=&capability=&path=`: would it be allowed, and which rule says so |
| `GET /api/security`, `PUT /api/security` | `security` | the sessions' lifetime |
| `GET /api/identity-providers`, `POST …` | `identity-providers` | |
| `PUT /api/identity-providers/{id}`, `DELETE …` | `identity-providers/{id}` | |
| `POST /api/identity-providers/test` | `identity-providers` | read the provider's discovery document |

## Federation

| Call | Policy path | |
|---|---|---|
| `GET /api/federation`, `PATCH /api/federation` | `federation` | this hub's role and name |
| `POST /api/federation/member`, `PATCH …`, `DELETE …` | `federation/member` | become, change, stop being a managed hub |
| `POST /api/federation/member/code` | `federation/member` | a new pairing code |
| `POST /api/federation/datacenters` | `federation/datacenters` | add a managed hub |
| `PATCH`/`DELETE /api/federation/datacenters/{id}` | `federation/datacenters/{id}` | |
| `ANY /api/dc/{id}/{path}` | `dc/{id}/{path}` | the same API of the managed hub, through the link: [Federation](../install/federation.md) |

## The agents' protocol

Agents do not use this API: they open one gRPC stream to the hub (`proto/infraviz/v1/agent.proto`, TLS or h2c, the same port): they send their topology and metrics, and receive commands (change version, read a log). The hub holds no connection into the cluster. The shape of the protocol is in [Architecture](architecture.md).
