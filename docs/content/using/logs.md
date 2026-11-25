# Container logs

Open the log of a pod or container from the topology: select it, **Open the log**, and a panel at the bottom of the page streams it as it is written (follow,
search, timestamps, wrap, download, the last 100 to 5000 lines). It stays open while you use the map above it, and closing it stops the reading.
This is for *why did it fail*: **Previous** shows what the container said before it was restarted.

## It is opt-in

Add source → **Allow reading container logs** (off by default). Logs can hold secrets, so nothing reads them unless the manifest of that source said so:

| | what the manifest adds |
|---|---|
| Kubernetes | `pods/log: get` in the agent's ClusterRole, and `LOGS=1` on the cluster reader (the node agents have no API to read it from) |
| Docker Swarm | `LOGS=1` on the agent service. A manager's agent reads the logs of every task through the Docker API; a worker's agent says it is not the one |
| Docker (one machine) | `LOGS=1`; it reads its own containers through the socket it already has |

A source that was added without it shows "Reading logs is off for this source" and how to change that (add it again, apply the manifest once). A source that
allows it shows a `logs` chip on its card.

## Who may read what

`GET /api/logs/<source id>/<namespace>/<name>`, so the policy path is `logs/<source id>/<namespace>/<name>` and the capability is `read`:

```hcl
path "logs/prod-k8s/shop/*"        { capabilities = ["read"] }
path "logs/prod-k8s/shop/secret-*" { capabilities = ["deny"] }
```

The panel is only offered to someone whose policies let them read that log. Every log that is opened is written to the hub's log (`audit: <user> reads the log of
<source>/<namespace>/<name> ...`).

## What it costs

Nothing is stored: the lines go from the agent through the hub to the browser. At most 16 logs are read at once over the hub, an agent reads at most 4, a followed log
ends after 30 minutes (open it again), and one request is cut at 8 MiB. A viewer that leaves, or is too slow, cancels the request at the agent.

## The log that is kept with an alert

A pod that is deleted takes its log with it, and the reason is in the last lines. So when an alert opens for a workload on a source that allows logs, the hub reads the
last 200 lines once (and, if the container was restarted, the ones before the restart) and keeps them with the alert: the incident report shows them under *The log of the
container*, and *Copy as Markdown* includes the last 80. They are kept as long as the incident is (Settings → Keep incident history).

## Limits

- The logs of a datacenter (a hub a master hub controls) are read on that hub: the master does not carry the stream.
- Docker keeps one log per container across restarts, so *Previous* on a Docker machine says there is no earlier one; on Swarm it is the task that ran the same slot before.
- Windows agents do not read logs yet.
