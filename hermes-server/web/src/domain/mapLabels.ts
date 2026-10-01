// The words on the map: what each thing says about itself. Pure functions of the state, so they are tested without drawing anything.
import type { HubState } from './hubState';
import { fmtCpu, fmtRateShort, fmtSize } from './format';
import type { Node } from './model';
import { PROVIDERS } from './providers';
import { ICON } from './status';
import { isSystem, kidsOf } from './selectors';

/** What a workload is using, under its name: "12m · 24Mi" (millicores, MiB), or nothing measurable. */
export function usageLabel(n: Node): string {
  if (n.m.cpuMilli != null) return `${fmtCpu(n.m.cpuMilli).replace(' cores', 'c')} · ${Math.round(n.m.memMiB ?? 0)}Mi`;
  if (n.m.cpu != null) return `cpu ${Math.round(n.m.cpu)}%`;
  return 'running';
}

export const shorten = (s: string, n: number): string => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/**
 * Does the volume have a size to be full of? A Kubernetes claim does; a local Docker volume has no limit of its own, and then all that is known is
 * how much it takes.
 */
export const hasCapacity = (n: Node): boolean => (n.meta.size ?? 0) > 0;

/** How full a volume is, 0..1; 0 when it has no size. */
export const volumeFill = (n: Node): number => (hasCapacity(n) ? Math.min(1, Math.max(0, (n.m.used ?? 0) / (n.meta.size ?? 1))) : 0);

/** What the volume symbol says: the percentage, or how much it takes when there is no size to take a percentage of. */
export function volumeBadge(n: Node): string {
  if (hasCapacity(n)) return `${Math.round(volumeFill(n) * 100)}%`;
  const used = n.m.used ?? 0;
  return used >= 1 ? `${used.toFixed(used < 10 ? 1 : 0)}G` : `${Math.round(used * 1024)}M`;
}

/** A compact "14/20 GiB" so that neighbours do not overlap. */
export function volumeSub(n: Node): string {
  const u = n.m.used ?? 0, s = n.meta.size ?? 0;
  if (!hasCapacity(n)) return `${fmtSize(u)} used`;
  return s >= 1024 ? `${(u / 1024).toFixed(2)}/${(s / 1024).toFixed(1)} TiB` : `${u.toFixed(u < 10 ? 1 : 0)}/${s} GiB`;
}

/** What a network says under its name: where it is reached, or its address range (or what kind it is)... */
export const networkSub = (n: Node): string => n.meta.addr || n.meta.subnet || n.meta.netKind || 'network';

/** Things that lead to (or apply to) others; a Docker network has apps "on" it. */
export const isGateway = (n: Node): boolean => ['service', 'nodeport', 'loadbalancer', 'ingress', 'gateway', 'httproute', 'policy', 'outside'].includes(n.meta.netKind ?? '');

/** ...and, on the line below, what kind it is and how many are on it (Docker) or behind it (a gateway): "overlay · 6", "service → 3". */
export function networkMembers(n: Node): string {
  const kind = n.meta.netKind ?? 'network';
  const members = n.meta.members ?? 0;
  return members > 0 ? `${kind} ${isGateway(n) ? '→' : '·'} ${members}` : kind;
}

export const NETWORK_ICONS: Record<string, string> = { outside: 'globe', ingress: 'gate', gateway: 'gate', httproute: 'fanout', service: 'fanout', nodeport: 'fanout', loadbalancer: 'balance', policy: 'shield' };

/** The icon of a network: one for each way in, the arrows for a Docker network. */
export const networkIcon = (n: Node): string => NETWORK_ICONS[n.meta.netKind ?? ''] ?? 'net';

export function nodeSub(n: Node): string {
  if (n.kind === 'network') return n.status === 'unknown' ? 'no data' : networkSub(n);
  if (n.kind === 'volume') return volumeSub(n);
  if (n.status === 'ok') return usageLabel(n);
  return n.status === 'unknown' ? 'no data' : n.reason || n.status;
}

/** "↓ received ↑ sent", when the agent measures it. */
export const nodeNet = (n: Node): string => (n.kind === 'network' ? networkMembers(n) : n.kind === 'volume' || n.status === 'unknown' || n.m.rxMbps == null ? '' : `↓${fmtRateShort(n.m.rxMbps)} ↑${fmtRateShort(n.m.txMbps ?? 0)}`);

export function nodeTooltip(s: HubState, n: Node): string {
  const kind = n.kind === 'volume' ? 'Volume (disk)' : (n.meta.type ?? n.kind);
  const ns = n.meta.ns ? `\nNamespace: ${n.meta.ns}${isSystem(s, n) ? ' (system)' : ''}` : '';
  return `${n.name} — ${kind}${ns}\nStatus: ${n.status}${n.reason ? ` (${n.reason})` : ''}${n.status === 'unknown' ? ' — its host is unreachable' : ''}`;
}

/** The line at the bottom of a host box: load, and what its pods move added up. */
export function hostStat(s: HubState, host: Node): string {
  if (host.status === 'crit') return '✕ UNREACHABLE';
  if (host.status === 'unknown') return '· NO DATA';
  if (host.m.cpu == null) return 'NO METRICS';
  const measured = kidsOf(s, host.id).filter((k) => k.kind === 'workload' && k.m.rxMbps != null);
  const net = measured.length ? ` · ↓${fmtRateShort(measured.reduce((a, k) => a + (k.m.rxMbps ?? 0), 0))} ↑${fmtRateShort(measured.reduce((a, k) => a + (k.m.txMbps ?? 0), 0))}` : '';
  return `CPU ${Math.round(host.m.cpu)}% · MEM ${Math.round(host.m.mem ?? 0)}%${net}`;
}

/** The corner of a cluster box: how many things are wrong, or what is missing. */
export function clusterStat(s: HubState, c: Node): string {
  const hosts = kidsOf(s, c.id);
  if (c.stale) {
    // with hosts still reporting their own heartbeat, say what is down; with none, all that can be said is that nothing is seen
    const alive = hosts.some((h) => h.kind === 'host' && !h.stale);
    if (!alive) return '✕ NO DATA';
    return c.provider === 'swarm' ? '✕ MANAGER DOWN' : '✕ CONTROL PLANE DOWN';
  }
  let issues = 0;
  for (const host of hosts) for (const n of [host, ...kidsOf(s, host.id)]) if (n.status === 'crit' || n.status === 'warn') issues++;
  return issues ? `${ICON[c.status]} ${issues} ${issues === 1 ? 'issue' : 'issues'}` : `${ICON.ok} Nominal`;
}

export const clusterSubtitle = (c: Node): string => (c.provider === 'storage' ? 'Shared network storage' : `${PROVIDERS[c.provider].label} · ${c.meta.version ?? ''}`);

/** How many moving dots a link gets: none when it is quiet or broken. */
export const particleCount = (mbps: number, control: boolean, broken: boolean): number => (broken ? 0 : control ? 1 : mbps < 0.3 ? 0 : mbps < 10 ? 1 : mbps < 40 ? 2 : mbps < 100 ? 3 : 4);

export const linkStrokeWidth = (mbps: number, control: boolean, broken: boolean): number => (broken ? 1.5 : control ? 1.6 : Number((1.3 + Math.log2(1 + mbps) * 0.3).toFixed(2)));
