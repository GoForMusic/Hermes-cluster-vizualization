// The TV's other "tab": a status wallboard (github status style: a card per host, a banner when something is wrong) instead of the
// topology map. Point a second screen at #/tv/status while the first stays on #/tv — each keeps whatever URL it was opened with.
import { dtg } from '../../domain/format';
import type { Alert, Node } from '../../domain/model';
import { hostsOf, recentIncidents, visibleClusters } from '../../domain/selectors';
import { ICON, STATUS_WORD } from '../../domain/status';
import { useWholeState } from '../../state/context';
import { useNow } from '../../ui/hooks';
import { Brand } from '../../ui/brand';
import { HeartbeatBar, StatusChip, TimeAgo } from '../../ui/status';

export function TvStatus() {
  const state = useWholeState();
  const now = useNow(1000);
  // Recomputed every tick, not just on the next hub event: a resolved incident ages out of the banner over real time.
  const alerts = recentIncidents(state, now);
  const crit = alerts.some((a) => a.resolvedTs == null && a.sev === 'crit');
  const d = new Date(now);
  const clusters = visibleClusters(state);
  const range = state.range;
  return (
    <div className={`tv tv-status${crit ? ' has-crit' : ''}`}>
      <header className="tv-top">
        <div className="seg tv-title"><Brand /></div>
        <div className="spacer" />
        <div className="seg tv-clock"><b>{d.toLocaleTimeString('en-GB', { hour12: false })}</b><span>DTG {dtg(d)}</span></div>
      </header>
      <div className="status-body">
        {alerts.length ? <IncidentBanner alerts={alerts} /> : <div className="status-nominal">✓ All systems operational</div>}
        {clusters.length
          ? clusters.map((c) => (
            <section key={c.id} className="status-cluster">
              <h2>{c.name}{c.status !== 'ok' ? <StatusChip status={c.status}>{STATUS_WORD[c.status]}</StatusChip> : null}</h2>
              <div className="status-grid">
                {hostsOf(state, c.id).map((h) => <HostCard key={h.id} host={h} bars={state.uptime.get(h.id)?.bars ?? []} pct={state.uptime.get(h.id)?.pct} rangeLabel={range.label} />)}
              </div>
            </section>
          ))
          : <div className="empty-map"><div><b>No data sources yet</b>Add a cluster in Admin → Sources.</div></div>}
      </div>
      <footer className="statusbar">
        <span className="hud">Status view</span>
        <span className="spacer" />
        <a href="#/tv">Map view ↗</a>
      </footer>
    </div>
  );
}

/**
 * `alerts` is active incidents plus ones that only just resolved (see `recentIncidents`). Still-active ones keep the alarming
 * red/yellow styling and the sort puts them first; a resolved one calms down to the "ok" colour and shows when it closed instead of
 * when it opened, the way a status page marks a closed incident "Resolved" rather than dropping it silently.
 */
function IncidentBanner({ alerts }: { alerts: readonly Alert[] }) {
  const active = alerts.filter((a) => a.resolvedTs == null);
  const worst = active.some((a) => a.sev === 'crit') ? 'crit' : active.length ? 'warn' : 'ok';
  const sorted = [...alerts].sort((a, b) => Number(a.resolvedTs != null) - Number(b.resolvedTs != null) || b.ts - a.ts);
  return (
    <div className={`status-banner st-${worst}`}>
      <div className="status-banner-h">
        {active.length ? (alerts.length > 1 ? `Incidents on ${alerts.length} monitors` : 'Incident') : 'Recently resolved'}
      </div>
      <div className="status-banner-list">
        {sorted.map((a) => {
          const resolved = a.resolvedTs != null;
          const sev = resolved ? 'ok' : a.sev === 'crit' ? 'crit' : 'warn';
          return (
            <div key={a.id} className={`status-banner-item${resolved ? ' resolved' : ''}`}>
              <b className={`st-${sev}`}>{ICON[sev]} {a.title}</b>
              <span className="muted">{a.detail}</span>
              {resolved ? <span className="muted">resolved <TimeAgo ts={a.resolvedTs!} /></span> : <TimeAgo ts={a.ts} />}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function HostCard({ host: h, bars, pct, rangeLabel }: { host: Node; bars: readonly string[]; pct?: number; rangeLabel: string }) {
  return (
    <div className={`status-card st-${h.status}`}>
      <div className="status-card-h">
        <span>{h.name}</span>
        <span className="status-card-ico">{ICON[h.status]}</span>
      </div>
      <HeartbeatBar bars={bars} />
      <div className="status-card-f muted mono">
        <span>{rangeLabel}</span>
        <span>{pct != null ? `${pct.toFixed(pct === 100 ? 0 : 2)}% uptime` : '—'}</span>
        <span>Now</span>
      </div>
      <div className="status-card-word">{STATUS_WORD[h.status]}</div>
    </div>
  );
}
