// Questions about a state. Pure functions: the components ask, they never dig in the state themselves.
import type { FlowLine } from '../generated/FlowLine';
import type { HubState } from './hubState';
import type { Alert, Edge, Node } from './model';
import { isRunning } from './status';

export const getNode = (s: HubState, id: string | null | undefined): Node | undefined => (id ? s.nodes.get(id) : undefined);
export const kidsOf = (s: HubState, id: string): Node[] => (s.kids.get(id) ?? []).flatMap((k) => s.nodes.get(k) ?? []);
export const allNodes = (s: HubState): Node[] => [...s.nodes.values()];
export const clusters = (s: HubState): Node[] => allNodes(s).filter((n) => n.kind === 'cluster');
export const hostsOf = (s: HubState, clusterId: string): Node[] => kidsOf(s, clusterId).filter((n) => n.kind === 'host');

export function clusterOf(s: HubState, id: string): Node | undefined {
  let n = s.nodes.get(id);
  while (n && n.kind !== 'cluster') n = n.parent ? s.nodes.get(n.parent) : undefined;
  return n;
}

/**
 * System pods are the cluster's own machinery, not your apps. They get their own colour and can be hidden; a system pod that is warning or
 * failing is never hidden, so hiding cannot hide a problem.
 */
export const isSystem = (s: HubState, n: Node): boolean =>
  n.provider === 'kubernetes' && (n.kind === 'workload' || n.kind === 'volume' || n.kind === 'network') && s.settings.systemNamespaces.includes(n.meta.ns ?? '');
export const isShown = (s: HubState, n: Node): boolean => !isSystem(s, n) || s.settings.showSystem || n.status === 'warn' || n.status === 'crit';
export const shownKids = (s: HubState, id: string): Node[] => kidsOf(s, id).filter((n) => isShown(s, n));
export const hiddenSystemCount = (s: HubState): number => allNodes(s).filter((n) => isSystem(s, n) && !isShown(s, n)).length;
export const isClusterVisible = (s: HubState, id: string): boolean => s.settings.clusters[id] !== false;
export const visibleClusters = (s: HubState): Node[] => clusters(s).filter((c) => isClusterVisible(s, c.id));
export const activeAlerts = (s: HubState): Alert[] => s.alerts.filter((a) => !a.resolvedTs);

/** Active alerts, plus ones resolved recently enough to still be worth a status banner — a real incident that just closed itself
 * should not vanish from view the instant it does, the way a status page keeps it visible for a while too. */
export const recentIncidents = (s: HubState, now: number, withinMs = 10 * 60 * 1000): Alert[] =>
  s.alerts.filter((a) => a.resolvedTs == null || now - a.resolvedTs < withinMs);

/** A node's id, its parent, its parent's parent, and so on up to (and including) its cluster. */
function withAncestors(s: HubState, id: string): Set<string> {
  const ids = new Set<string>();
  let n = s.nodes.get(id);
  while (n && !ids.has(n.id)) {
    ids.add(n.id);
    n = n.parent ? s.nodes.get(n.parent) : undefined;
  }
  return ids;
}

/** A node's id and everything under it (its workloads/volumes, a host's under a cluster, and so on). Leaf nodes just get themselves. */
function withDescendants(s: HubState, id: string): Set<string> {
  const ids = new Set<string>([id]);
  const stack = [id];
  while (stack.length) {
    const cur = stack.pop()!;
    for (const kid of s.kids.get(cur) ?? []) if (!ids.has(kid)) { ids.add(kid); stack.push(kid); }
  }
  return ids;
}

/**
 * The incidents that explain a colored cell in this node's heartbeat bar over `[sinceMs, nowMs]`: this node's own alerts, its
 * ancestors' (a workload's own bar can turn red only because its host went down — the alert is opened on the host, not on it, so
 * without this a pod's incident history would always look empty even when the map clearly shows why it was red), and its
 * descendants' (a host showing amber because one of its pods is struggling is explained by that pod's own alert, not one of its own).
 */
/**
 * How bad `alert` looks from `nodeId`'s own point of view — the same colour everywhere except one case: an alert inherited from a
 * descendant (a struggling pod explaining its host's cell) never outranks what that descendant can actually do to `nodeId`, which is
 * amber at most (see `deriveStatuses`'s host rollup) — a red alert two levels down does not make the host itself unreachable. An
 * alert on the node itself, or inherited from an ancestor (a host's real crit does make everything under it red), keeps its own
 * severity unchanged.
 */
export function effectiveSeverity(s: HubState, nodeId: string, alert: Alert): 'warn' | 'crit' {
  const sev = alert.sev === 'crit' ? 'crit' : 'warn';
  if (alert.nodeId === nodeId || !withDescendants(s, nodeId).has(alert.nodeId)) return sev;
  return sev === 'crit' ? 'warn' : sev;
}

export function incidentsFor(s: HubState, nodeId: string, sinceMs: number, nowMs: number): Alert[] {
  const ids = withAncestors(s, nodeId);
  for (const id of withDescendants(s, nodeId)) ids.add(id);
  return s.alerts
    .filter((a) => ids.has(a.nodeId) && a.ts <= nowMs && (a.resolvedTs == null || a.resolvedTs >= sinceMs))
    .slice()
    .sort((a, b) => b.ts - a.ts);
}

/** One row of a monitor list: a cluster heading (-1), one of its hosts (0), or a host's workload/volume (1). */
export interface MonitorRow {
  n: Node;
  level: -1 | 0 | 1;
}

/**
 * Every cluster in `clusterList`, its hosts, and each host's shown children (workloads, volumes) — flattened and indented. Callers pick
 * which clusters to include: `clusters(s)` (the dashboard shows everything) or `visibleClusters(s)` (the TV honours the per-cluster
 * toggle in TV display settings).
 */
export function monitorRows(s: HubState, clusterList: readonly Node[]): MonitorRow[] {
  return clusterList.flatMap((c) => [
    { n: c, level: -1 as const },
    ...hostsOf(s, c.id).flatMap((host) => [
      { n: host, level: 0 as const },
      ...shownKids(s, host.id).map((k) => ({ n: k, level: 1 as const })),
    ]),
  ]);
}

/** A link is broken when either end is down or unknown: there is no traffic to draw. */
export function edgeBroken(s: HubState, e: Edge): boolean {
  const down = (n: Node): boolean => n.status === 'unknown' || (n.kind !== 'volume' && n.status === 'crit');
  const a = s.nodes.get(e.from);
  const b = s.nodes.get(e.to);
  return !a || !b || down(a) || down(b);
}

/**
 * A rate the hub deduced and did not measure: what an ingress controller exchanges with the pods behind a Service is taken as the traffic of the
 * Ingress that sends there, and the same amount as what came in from outside for it.
 */
export function isDeduced(s: HubState, e: Edge): boolean {
  const from = s.nodes.get(e.from)?.meta.netKind, to = s.nodes.get(e.to)?.meta.netKind;
  return from === 'ingress' || (from === 'outside' && to === 'ingress');
}

/**
 * A volume is joined to the workloads that mount it. `mountedBy` names them (pods by name, or a Swarm service: its tasks are `service.1`,
 * `service.2`...), and a volume is on the host of the workloads that use it, so only the workloads of that host are looked at.
 */
export function mountLinks(s: HubState): Edge[] {
  const out: Edge[] = [];
  for (const v of s.nodes.values()) {
    if (v.kind !== 'volume' || !v.parent || !v.meta.mountedBy || !isShown(s, v)) continue;
    const names = v.meta.mountedBy.split(',').map((n) => n.trim()).filter(Boolean);
    for (const w of shownKids(s, v.parent)) {
      if (w.kind === 'workload' && names.some((n) => w.name === n || w.name.startsWith(`${n}.`))) {
        out.push({ id: `mount:${w.id}>${v.id}`, from: w.id, to: v.id, base: 0, mbps: 0, type: 'route' });
      }
    }
  }
  return out;
}

/** A list of flows older than this is not shown: its agent has stopped (or the source is gone). */
export const FLOWS_FRESH_MS = 30_000;

/** The busiest connections of every source, biggest first. */
export function topFlows(s: HubState, now: number, limit = 6): FlowLine[] {
  return [...s.flows.values()]
    .filter((f) => now - f.at < FLOWS_FRESH_MS)
    .flatMap((f) => f.lines)
    .sort((a, b) => b.mbps - a.mbps)
    .slice(0, limit);
}

/** What the volumes add up to: how much is used and how much is provisioned (only the ones that have a size), and how many there are. */
export function volumeTotals(s: HubState): { count: number; used: number; size: number; sized: number } {
  let count = 0, used = 0, size = 0, sized = 0;
  for (const n of s.nodes.values()) {
    if (n.kind !== 'volume' || !isShown(s, n)) continue;
    count++;
    used += n.m.used ?? 0;
    if ((n.meta.size ?? 0) > 0) { size += n.meta.size ?? 0; sized++; }
  }
  return { count, used, size, sized };
}

export interface Summary {
  clusters: number;
  hostsUp: number;
  hostsTotal: number;
  wlRun: number;
  wlTotal: number;
  traffic: number;
  trafficKnown: boolean;
  volumes: number;
  /** The fullest volume that has a size. An average would hide one nearly full volume among many empty ones. */
  fullest: { name: string; pct: number } | null;
  /** GiB in use over every volume, sized or not. */
  volumeUsed: number;
  crit: number;
  warn: number;
}

export function summary(s: HubState): Summary {
  let hostsUp = 0, hostsTotal = 0, wlRun = 0, wlTotal = 0, volumes = 0, volumeUsed = 0, clusterCount = 0, sent = 0;
  let fullest: Summary['fullest'] = null;
  let measured = false;
  for (const n of s.nodes.values()) {
    if (n.kind === 'cluster') clusterCount++;
    if (n.kind === 'host') {
      hostsTotal++;
      if (n.status !== 'crit' && n.status !== 'unknown') hostsUp++;
    }
    if (n.kind === 'workload' && isShown(s, n)) { // what is drawn: hidden system pods are not counted
      wlTotal++;
      if (isRunning(n.status)) wlRun++;
    }
    if (n.kind === 'workload' && n.m.txMbps != null) {
      measured = true;
      sent += n.m.txMbps;
    }
    if (n.kind === 'volume' && isShown(s, n)) {
      volumes++;
      volumeUsed += n.m.used ?? 0;
      const size = n.meta.size ?? 0;
      const pct = size > 0 ? ((n.m.used ?? 0) / size) * 100 : null; // a volume with no size has no percentage
      if (pct != null && (fullest == null || pct > fullest.pct)) fullest = { name: n.name, pct };
    }
  }
  // Real agents measure what each pod/container sends (counted once, at the sender); the demo has drawn links instead.
  // With neither, the figure is unknown, not zero.
  const linkTraffic = s.edges.reduce((sum, e) => sum + (e.type !== 'traffic' || edgeBroken(s, e) ? 0 : e.mbps), 0);
  const active = activeAlerts(s);
  return {
    clusters: clusterCount, hostsUp, hostsTotal, wlRun, wlTotal,
    traffic: measured ? sent : linkTraffic,
    trafficKnown: measured || s.edges.some((e) => e.type === 'traffic'),
    volumes, fullest, volumeUsed,
    crit: active.filter((a) => a.sev === 'crit').length,
    warn: active.filter((a) => a.sev === 'warn').length,
  };
}

export const series = (s: HubState, key: string): readonly number[] => s.history.get(key) ?? [];
