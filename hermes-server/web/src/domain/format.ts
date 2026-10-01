// Numbers and times as people read them.

export function fmtSize(gib: number): string {
  if (gib >= 1024) return `${(gib / 1024).toFixed(2)} TiB`;
  return `${gib.toFixed(gib < 10 ? 1 : 0)} GiB`;
}

/** kubectl-top style: pods are measured in absolute terms (millicores, MiB), not as a share of the node. */
export const fmtCpu = (milli: number): string => (milli >= 1000 ? `${(milli / 1000).toFixed(2)} cores` : `${Math.round(milli)}m`);
export const fmtMem = (mib: number): string => (mib >= 1024 ? `${(mib / 1024).toFixed(1)} GiB` : `${Math.round(mib)} MiB`);

/** Compact throughput for map labels: "12K" is 12 Kb/s, "1.2M" is 1.2 Mb/s. */
export function fmtRateShort(v: number): string {
  if (v < 0.001) return '0';
  if (v < 1) return `${Math.round(v * 1000)}K`;
  return v < 10 ? `${v.toFixed(1)}M` : `${Math.round(v)}M`;
}

export function fmtMbps(v: number): string {
  if (v < 1) return `${Math.round(v * 1000)} Kb/s`;
  if (v >= 1000) return `${(v / 1000).toFixed(1)} Gb/s`;
  if (v >= 10) return `${Math.round(v)} Mb/s`;
  return `${v.toFixed(1)} Mb/s`;
}

export function timeAgo(ts: number, now: number): string {
  const s = Math.max(0, (now - ts) / 1000);
  if (s < 60) return `${Math.floor(s)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

export function fmtDur(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

const pad2 = (n: number): string => String(n).padStart(2, '0');
const MONTHS = ['JAN', 'FEB', 'MAR', 'APR', 'MAY', 'JUN', 'JUL', 'AUG', 'SEP', 'OCT', 'NOV', 'DEC'];

/** Military date-time group, e.g. 201453Z SEP 26 (UTC). */
export function dtg(d: Date): string {
  return `${pad2(d.getUTCDate())}${pad2(d.getUTCHours())}${pad2(d.getUTCMinutes())}Z ${MONTHS[d.getUTCMonth()]} ${String(d.getUTCFullYear()).slice(2)}`;
}

export function hhmmss(ts: number): string {
  const d = new Date(ts);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

export const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
