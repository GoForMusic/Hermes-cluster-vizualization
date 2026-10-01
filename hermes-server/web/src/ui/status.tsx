// Status display: chips, map symbols, alert cards, heartbeat bars and stat tiles — everything that shows an ok/warn/crit status.
// Take props and draw: no fetching, no store.
import type { ReactNode } from 'react';
import type { Alert, Status } from '../domain/model';
import { fmtDur, timeAgo } from '../domain/format';
import { ICON, LABEL } from '../domain/status';
import { useNow } from './hooks';

export function StatusChip({ status, children }: { status: Status; children?: ReactNode }) {
  return (
    <span className={`chip st-${status}`}>
      <i>{ICON[status]}</i>
      {children ?? LABEL[status]}
    </span>
  );
}

/** Uptime Kuma style bar: one cell per time bucket (up | warn | down | nodata). */
/** `onPick`: makes each cell clickable, reporting its index into the original (un-sliced) `bars`. `picked`: which one, if any, is
 * shown selected — both optional, so every other caller of this bar (the sidebar's small ones, the TV's) is unaffected. */
export function HeartbeatBar({ bars = [], small = false, take = 0, onPick, picked }: { bars?: readonly string[]; small?: boolean; take?: number; onPick?: (i: number) => void; picked?: number | null }) {
  const list = take ? bars.slice(-take) : bars;
  const offset = bars.length - list.length; // `take` may have sliced off the older end; shift indices back to the full `bars`
  return (
    <div className={`hb${small ? ' sm' : ''}${onPick ? ' pickable' : ''}`}>
      {list.map((b, i) => <i key={i} className={`${b}${picked === i + offset ? ' picked' : ''}`} onClick={onPick ? () => onPick(i + offset) : undefined} />)}
    </div>
  );
}

export function StatTile({ label, value, unit, sub, status = 'ok', children }: { label: string; value: ReactNode; unit?: string; sub?: string; status?: Status; children?: ReactNode }) {
  return (
    <div className={`tile st-${status}`}>
      <div className="tile-label">{label}</div>
      <div className="tile-value">{value}{unit ? <small> {unit}</small> : null}</div>
      <div className="tile-sub">{sub ?? ''}</div>
      {children ? <div className="tile-spark">{children}</div> : null}
    </div>
  );
}

/** Status colors, shared by the map's mini symbols and the legend so they can never drift apart from each other. */
export const STATUS_FILL = { ok: '#80e0ff', system: '#8fd694', warn: '#ffec80', crit: '#ff8a84', unknown: '#4b5665' } as const;

/** A tiny APP-6 style unit symbol: a rectangle when fine, a diamond when failing. */
export function MiniSymbol({ status, system = false }: { status: Status; system?: boolean }) {
  const fill = status === 'ok' && system ? STATUS_FILL.system : STATUS_FILL[status];
  return (
    <svg className="sym-mini" viewBox="0 0 20 15" aria-hidden="true">
      {status === 'crit'
        ? <polygon points="10,0.5 19.5,7.5 10,14.5 0.5,7.5" fill={fill} stroke="#05090e" strokeWidth={1.2} />
        : <rect x={1.5} y={1.5} width={17} height={12} fill={fill} stroke={status === 'unknown' ? '#7d8a9b' : '#05090e'} strokeWidth={1.4} strokeDasharray={status === 'unknown' ? '3 2' : undefined} />}
    </svg>
  );
}

/** "5s", refreshed every second. */
export function TimeAgo({ ts }: { ts: number }) {
  return <span className="alert-time">{timeAgo(ts, useNow())}</span>;
}

/** `context`: named when the alert belongs to an ancestor (a workload's red cell usually comes from its host) — without it, an
 * incident inherited from the host/cluster looks like it happened to this node itself. */
/** `sev`: overrides the alert's own severity for how it looks here — an inherited incident can look less severe from where it is
 * shown than it truly is (see `effectiveSeverity`), without changing the alert itself. Defaults to the alert's own. */
export function AlertCard({ alert, onAck, onOpen, context, sev: sevOverride }: { alert: Alert; onAck?: (id: number) => void; /** Opens the incident report; the title becomes a button. */ onOpen?: (alert: Alert) => void; context?: string; sev?: 'warn' | 'crit' }) {
  const resolved = alert.resolvedTs != null;
  const sev = sevOverride ?? (alert.sev === 'crit' ? 'crit' : 'warn');
  return (
    <div className={`alert st-${sev}${resolved ? ' resolved' : ''}`}>
      <div className="alert-ico">{ICON[sev]}</div>
      <div className="alert-body">
        <div className="alert-title">
          {onOpen ? <button type="button" className="linklike" onClick={() => onOpen(alert)} title="Open the incident report">{alert.title}</button> : alert.title}
          {context ? <span className="muted"> · {context}</span> : null}
        </div>
        <div className="alert-detail">{alert.detail}</div>
      </div>
      <div className="alert-side">
        <TimeAgo ts={alert.ts} />
        {resolved ? <span className="alert-tag">resolved · {fmtDur(alert.resolvedTs! - alert.ts)}</span> : null}
        {onAck && !resolved && !alert.ack ? <button className="btn xs" onClick={() => onAck(alert.id)}>Ack</button> : null}
        {alert.ack && !resolved ? <span className="alert-tag">acked{alert.ackBy ? ` · ${alert.ackBy}` : ''}</span> : null}
      </div>
    </div>
  );
}
