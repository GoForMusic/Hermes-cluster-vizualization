// The effective status of every node. A source reports what it sees (`own`); what the app shows (`status`) also depends on what is known
// about the host and about the source: everything on a crashed host is unknown, and so is everything of a source that cannot be reached.
import type { Node, Status } from './model';
import type { Settings } from './settings';
import { RANK } from './status';

type Nodes = ReadonlyMap<string, Node>;
type Kids = ReadonlyMap<string, readonly string[]>;

const kidsOf = (nodes: Nodes, kids: Kids, id: string): Node[] => (kids.get(id) ?? []).flatMap((k) => nodes.get(k) ?? []);

/** Returns the map with `own`, `reason`, `status` and `since` brought up to date. A node that did not change keeps its identity. */
export function deriveStatuses(nodes: Nodes, kids: Kids, settings: Settings, now: number): Map<string, Node> {
  const out = new Map(nodes);
  const rule = settings.rules.find((r) => r.id === 'volume-usage');
  const warnAt = rule?.value ?? 85;
  const critAt = rule?.crit ?? 95;

  // a volume is as healthy as it is empty
  for (const n of nodes.values()) {
    if (n.kind !== 'volume') continue;
    const size = n.meta.size ?? 0;
    const pct = size ? ((n.m.used ?? 0) / size) * 100 : 0;
    const own = pct >= critAt ? 'crit' : pct >= warnAt ? 'warn' : 'ok';
    const reason = own === 'ok' ? '' : `${pct.toFixed(0)}% used`;
    if (own !== n.own || reason !== n.reason) out.set(n.id, { ...n, own, reason });
  }

  // The status every node should show, worked out once from raw facts (own/stale/parent) rather than from what it showed last time:
  // a host's status below folds in its workloads, so basing this pass on the previous `status` instead of `own` would make an
  // already-folded-in warning look like a fresh change forever, bumping `since` and breaking a node's identity on every call.
  const status = new Map<string, Status>();
  for (const n of out.values()) {
    if (n.kind === 'cluster') continue;
    let st: Status = n.own;
    const parent = n.kind !== 'host' && n.parent ? nodes.get(n.parent) : undefined;
    if (parent?.own === 'crit') st = 'unknown';
    // A Docker machine is described by its own agent and nobody else: when that agent goes quiet nothing says its containers still run.
    // (On Kubernetes the API still vouches for the pods of a node whose agent is silent, so there it stays as it is.)
    if (parent?.stale && n.provider === 'docker') st = 'unknown';
    // its source cannot be reached: what we hold is the last known state, not the current one (a host last seen down stays down)
    if (n.stale && st !== 'crit') st = 'unknown';
    status.set(n.id, st);
  }

  // A host whose own heartbeat is fine but is running a struggling workload (a pod stuck pending, a crashing container) is not fully
  // healthy either: it shows amber for that. It never shows red for it, though — "Down" stays reserved for the host's own heartbeat,
  // since one bad pod does not mean the machine is unreachable.
  for (const h of out.values()) {
    if (h.kind !== 'host') continue;
    let worstChild: Status = 'ok';
    for (const n of kidsOf(out, kids, h.id)) {
      const s = status.get(n.id)!;
      if (s !== 'unknown' && RANK[s] > RANK[worstChild]) worstChild = s;
    }
    const capped = worstChild === 'crit' ? 'warn' : worstChild;
    if (RANK[capped] > RANK[status.get(h.id)!]) status.set(h.id, capped);
  }

  for (const c of out.values()) {
    if (c.kind !== 'cluster') continue;
    let worst: Status = 'ok';
    const hosts = kidsOf(out, kids, c.id);
    for (const host of hosts) {
      for (const n of [host, ...kidsOf(out, kids, host.id)]) {
        const s = status.get(n.id)!;
        if (s !== 'unknown' && RANK[s] > RANK[worst]) worst = s;
      }
    }
    // A cluster whose source cannot be reached is down as far as we can tell (red). If some of its hosts still vouch for themselves it is
    // degraded, not off: the workloads on them normally keep running without the control plane (amber).
    const alive = hosts.some((h) => h.kind === 'host' && !h.stale);
    let st: Status = c.stale ? (alive ? 'warn' : 'crit') : worst;
    // Some hosts down among others that are fine is a partial outage (amber), not the whole cluster being down (red) — that is for
    // when every host is unreachable. A workload/volume being critical while every host is fine is unaffected: that was never "some
    // hosts down" to begin with.
    const hostsDown = hosts.filter((h) => h.own === 'crit');
    if (!c.stale && st === 'crit' && hostsDown.length > 0 && hostsDown.length < hosts.length) st = 'warn';
    status.set(c.id, st);
  }

  for (const [id, st] of status) {
    const n = out.get(id)!;
    if (st !== n.status) out.set(id, { ...n, status: st, since: now });
  }
  return out;
}
