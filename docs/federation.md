# Federation: one hub that manages another

Agents only ever connect **out** to a hub. When a firewall lets traffic go from zone A to zone B but not back, the agents in zone B cannot
reach a hub in zone A. Federation fixes that without opening the firewall: run a second hub in zone B (a **member**), and let the hub in
zone A (the **master**) connect to it. The master dials the member, which is the direction the firewall allows; the agents in zone B still
connect only to the hub next to them.

```
  zone A                              zone B
 ┌───────────┐    master dials     ┌───────────┐ <── agents (outbound, local)
 │ hub (dc1) │ ──────────────────> │ hub (dc2) │ <── agents
 │  master   │ (allowed by the FW) │  member   │
 └───────────┘                     └───────────┘
```

What the master gets: the member's sources, nodes, traffic and alerts on its own map and TV (a node that is unreachable is shown out of date,
never as healthy), and the **whole admin side**: Add source, rename, remove, Change agent version and the registry of that datacenter are all
done from the master. The member has no login of its own.

## Setting it up

1. **On the member**, at the first start (or later in Settings → Federation → "Let another hub manage this one"), choose *Managed by another hub*
   and give the address the master and the agents will use to reach it. The hub shows a **pairing code** (valid for one hour, usable once).
   It is the same compose / Helm install as any hub.
2. **On the master**, Settings → Federation → *Add datacenter*: a name, the member's address and the code. The master connects, the code is used
   up, and the member answers with a link token that only the master keeps (stored encrypted; the member keeps only a hash).
3. The member **locks**: its login is off and its page only says *Managed by dc2*. From now on everything is done from the master.

### Two hubs that already run

Nothing has to be reinstalled. On the hub that should become the child, log in as its admin and open **Settings → Federation → "Let another hub
manage this one…"**; it shows the address and the pairing code and keeps working as before until the parent connects. Then add it on the parent as
above. Its sources, history and registry stay (the parent sees its sources and manages them), its accounts stop working (users live only on the
parent), and two sources with the same name in the two hubs do not clash: each is tagged with its datacenter. Only the hub that was chosen as the
child loses its login; the parent is untouched. Remove the datacenter on the parent to give the child its login back.

Each hub has its own `HUB_SECRET_KEY`, and the member keeps its own SQLite file; the master does not hold the member's registry password.

## What runs where

| | Master | Member |
|---|---|---|
| Users, logins, TV and admin pages | yes | none |
| Add source / rename / remove / change agent version | the buttons | executes (the master's call is carried over the link) |
| Registry login | one per datacenter (Settings → Registry → "For the agents of") | stored here, encrypted; *Test connection* and the version list run here, so a registry that only the member's zone can reach works |
| Agents | connect to the member's address | connect here |
| Alerts | computed from the topology it mirrors | computed here too |

The master may only make these calls on a member: `sources` (list, add, rename, remove, upgrade), `registry` (get, save, test, versions) and the
member's agent address. No logins, no settings, and releasing is a message of its own.

## The link

One bidirectional gRPC stream (`proto/infraviz/v1/federation.proto`, same compatibility rules as the agent contract). The member sends its
sources, their topologies and live changes; the master sends admin requests. If it drops, the master keeps what it saw, marked out of date,
shows the datacenter as *unreachable* and its sources as errors (so the usual "source unreachable" alert fires), and dials again with a
back-off. Requests made meanwhile fail clearly; nothing is queued.

Over `https://`, the master trusts the usual public roots; add a private CA with `HUB_FEDERATION_CA_FILE` (PEM). A plain `http://` address works
too, for a trusted network.

## Getting a member back

- **Remove the datacenter on the master** (Settings → Federation → Remove). The member is told to let go and is a hub on its own again (it asks
  for a first admin).
- **The master is gone for good, or the member could not be told**: restart the member once with `HUB_FEDERATION=off`, then remove the variable.
  This needs access to the machine, which is the point.

## Not in this version

Alert acknowledgement stays on the master (users live there); more than two levels, or one member with several masters; a datacenter column on
the map. A member's `HUB_AGENT_IMAGE` and the like are still its own environment.
