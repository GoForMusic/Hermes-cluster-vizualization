// The incident report: what one alert amounts to, told from what the hub kept (the alert, the node as it was when the alert opened) and
// from what is known now (where the node sits, what else was down at the same time). Pure: the dialog draws it, the page exports it.
import { fmtDur } from './format';
import type { HubState } from './hubState';
import type { Alert, ContainerInfo } from './model';
import { getNode } from './selectors';

/** What the node looked like when the alert opened, as the hub stored it. */
export interface NodeSnapshot {
  own?: string;
  reason?: string;
  m?: Record<string, number>;
  meta?: Record<string, unknown>;
}

export function parseSnapshot(raw: string): NodeSnapshot | null {
  if (!raw) return null;
  try {
    const v: unknown = JSON.parse(raw);
    return v && typeof v === 'object' ? (v as NodeSnapshot) : null;
  } catch { return null; }
}

/** The source an id belongs to: node ids start with it (`s1a2b3c:n:host`); the alert of a whole source is `source:s1a2b3c`. */
export const sourceOf = (nodeId: string): string => nodeId.replace(/^source:/, '').split(':')[0] ?? '';

export interface IncidentFacts {
  /** From the cluster down to the node, by name; only the id when the node is gone. */
  path: string[];
  gone: boolean;
  /** The same source's other incidents that overlapped this one in time: what else was down while it lasted. */
  overlapping: Alert[];
  snapshot: NodeSnapshot | null;
  /** Milliseconds it lasted, or has lasted so far. */
  duration: number;
  active: boolean;
}

export function incidentFacts(state: HubState, alert: Alert, now: number): IncidentFacts {
  const path: string[] = [];
  let n = getNode(state, alert.nodeId);
  const gone = !n;
  while (n) {
    path.unshift(n.name);
    n = n.parent ? getNode(state, n.parent) : undefined;
  }
  if (gone) path.push(alert.nodeId.startsWith('source:') ? 'the whole source' : alert.nodeId);
  const end = alert.resolvedTs ?? now;
  const src = sourceOf(alert.nodeId);
  const overlapping = state.alerts
    .filter((o) => o.id !== alert.id && sourceOf(o.nodeId) === src && o.ts <= end && (o.resolvedTs ?? now) >= alert.ts)
    .sort((a, b) => a.ts - b.ts);
  return { path, gone, overlapping, snapshot: parseSnapshot(alert.snapshot), duration: end - alert.ts, active: alert.resolvedTs == null };
}

/** The scalar facts of a snapshot worth showing (image, restarts, phase…), in a stable order, as label/value pairs. */
export function snapshotFacts(s: NodeSnapshot): [string, string][] {
  const out: [string, string][] = [];
  const meta = s.meta ?? {};
  const put = (label: string, v: unknown) => { if (v != null && v !== '' && typeof v !== 'object') out.push([label, String(v)]); };
  put('Status then', s.own);
  put('Reason then', s.reason);
  for (const [k, label] of [['type', 'Kind'], ['ns', 'Namespace'], ['phase', 'Phase'], ['image', 'Image'], ['restarts', 'Restarts'], ['ip', 'Address'], ['role', 'Role']] as const) put(label, meta[k]);
  const m = s.m ?? {};
  if (m.cpu != null) put('CPU then', `${m.cpu.toFixed(1)}%`);
  if (m.mem != null) put('Memory then', `${m.mem.toFixed(1)}%`);
  if (m.cpuMilli != null) put('CPU then', `${Math.round(m.cpuMilli)}m`);
  if (m.memMiB != null) put('Memory then', `${Math.round(m.memMiB)} MiB`);
  return out;
}

export const snapshotContainers = (s: NodeSnapshot): ContainerInfo[] => (Array.isArray(s.meta?.containers) ? (s.meta!.containers as ContainerInfo[]) : []);

const when = (ts: number): string => new Date(ts).toLocaleString('en-GB', { hour12: false });

/** One incident, ready to paste in a ticket. */
export function incidentMarkdown(alert: Alert, f: IncidentFacts): string {
  const lines = [
    `## ${alert.sev === 'crit' ? 'CRITICAL' : 'WARNING'}: ${alert.title}`,
    '',
    `- **Where:** ${f.path.join(' › ')}`,
    `- **Started:** ${when(alert.ts)}`,
    f.active ? `- **Still open:** ${fmtDur(f.duration)} so far` : `- **Resolved:** ${when(alert.resolvedTs!)} (lasted ${fmtDur(f.duration)})`,
    alert.ack ? `- **Acknowledged:** ${alert.ackBy || 'someone'}${alert.ackTs ? `, ${when(alert.ackTs)}` : ''}` : '- **Acknowledged:** no',
  ];
  if (alert.detail) lines.push('', '**What the hub saw**', '', alert.detail);
  if (f.snapshot) {
    const facts = snapshotFacts(f.snapshot);
    if (facts.length) lines.push('', '**The node when it opened**', '', ...facts.map(([k, v]) => `- ${k}: ${v}`));
    const cs = snapshotContainers(f.snapshot);
    if (cs.length) lines.push('', '**Containers**', '', ...cs.map((c) => `- ${c.name} (${c.image}): ${c.state}, ${c.restarts} restart${c.restarts === 1 ? '' : 's'}${c.ready ? '' : ', not ready'}`));
  }
  if (f.overlapping.length) lines.push('', '**Also down meanwhile**', '', ...f.overlapping.map((o) => `- ${o.title} (${when(o.ts)}${o.resolvedTs ? `, ${fmtDur(o.resolvedTs - o.ts)}` : ', still open'})`));
  return lines.join('\n');
}

/** A list of incidents, oldest first, as a report. */
export function incidentsMarkdown(alerts: readonly Alert[], state: HubState, now: number, heading: string): string {
  const sorted = alerts.slice().sort((a, b) => a.ts - b.ts);
  const crit = sorted.filter((a) => a.sev === 'crit').length;
  const head = [`# ${heading}`, '', `${sorted.length} incident${sorted.length === 1 ? '' : 's'} (${crit} critical, ${sorted.length - crit} warning) · ${sorted.filter((a) => a.resolvedTs == null).length} still open · generated ${when(now)}`, ''];
  return [...head, ...sorted.map((a) => incidentMarkdown(a, incidentFacts(state, a, now))).flatMap((block) => [block, ''])].join('\n');
}
