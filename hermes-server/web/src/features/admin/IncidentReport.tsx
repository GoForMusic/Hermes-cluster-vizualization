// The incident report: what happened, where, from when to when, who noticed, what the node looked like when it opened and what else was
// down meanwhile. Opened from any alert; "Copy as Markdown" is what goes in the ticket.
import { useState } from 'react';
import { fmtDur } from '../../domain/format';
import { incidentFacts, incidentMarkdown, snapshotContainers, snapshotFacts } from '../../domain/incidents';
import type { Alert } from '../../domain/model';
import { useStore, useWholeState } from '../../state/context';
import { Dialog } from '../../ui/dialog';
import { StatusChip } from '../../ui/status';

const when = (ts: number): string => new Date(ts).toLocaleString('en-GB', { hour12: false });

export function IncidentReport({ alert, onClose, onOpen }: { alert: Alert | null; onClose: () => void; onOpen: (a: Alert) => void }) {
  const state = useWholeState();
  const store = useStore();
  const [copied, setCopied] = useState(false);
  // the alert in the list may have changed since it was clicked (acknowledged, resolved): show the newest
  const a = alert ? state.alerts.find((x) => x.id === alert.id) ?? alert : null;
  const f = a ? incidentFacts(state, a, Date.now()) : null;
  const facts = f?.snapshot ? snapshotFacts(f.snapshot) : [];
  const containers = f?.snapshot ? snapshotContainers(f.snapshot) : [];
  const close = () => { setCopied(false); onClose(); };

  return (
    <Dialog open={a != null} onClose={close} wide>
      {a && f ? (
        <div className="report">
          <div className="src-top">
            <h2 style={{ margin: 0 }}>{a.title}</h2>
            <StatusChip status={a.sev === 'crit' ? 'crit' : 'warn'}>{a.sev === 'crit' ? 'Critical' : 'Warning'}</StatusChip>
          </div>
          <dl className="report-facts">
            <dt>Where</dt><dd>{f.path.join(' › ')}{f.gone ? <span className="muted"> · no longer in the topology</span> : null}</dd>
            <dt>Started</dt><dd>{when(a.ts)}</dd>
            <dt>{f.active ? 'Open for' : 'Resolved'}</dt><dd>{f.active ? `${fmtDur(f.duration)} so far` : `${when(a.resolvedTs!)} · lasted ${fmtDur(f.duration)}`}</dd>
            <dt>Acknowledged</dt><dd>{a.ack ? `${a.ackBy || 'someone'}${a.ackTs ? `, ${when(a.ackTs)}` : ''}` : <>no {f.active ? <button className="btn xs" style={{ marginLeft: 8 }} onClick={() => void store.ackAlert(a.id)}>Acknowledge</button> : null}</>}</dd>
          </dl>
          {a.detail ? <><h3>What the hub saw</h3><p className="report-detail">{a.detail}</p></> : null}
          {facts.length || containers.length ? (
            <>
              <h3>The node when it opened</h3>
              <p className="muted" style={{ marginTop: 0, fontSize: 12 }}>Kept at that moment: the pod or host may be gone by now.</p>
              {facts.length ? <dl className="report-facts">{facts.map(([k, v]) => <div key={k} style={{ display: 'contents' }}><dt>{k}</dt><dd className="mono">{v}</dd></div>)}</dl> : null}
              {containers.length ? (
                <ul className="report-list">
                  {containers.map((c) => <li key={c.name}><b>{c.name}</b> <span className="mono muted">{c.image}</span> · {c.state} · {c.restarts} restart{c.restarts === 1 ? '' : 's'}{c.ready ? '' : ' · not ready'}</li>)}
                </ul>
              ) : null}
            </>
          ) : null}
          {f.overlapping.length ? (
            <>
              <h3>Also down meanwhile</h3>
              <ul className="report-list">
                {f.overlapping.map((o) => (
                  <li key={o.id}><button type="button" className="linklike" onClick={() => onOpen(o)}>{o.title}</button> <span className="muted">· {when(o.ts)}{o.resolvedTs ? ` · ${fmtDur(o.resolvedTs - o.ts)}` : ' · still open'}</span></li>
                ))}
              </ul>
            </>
          ) : null}
          <div className="dlg-actions">
            <button className="btn" onClick={() => { void navigator.clipboard?.writeText(incidentMarkdown(a, f)); setCopied(true); }}>{copied ? 'Copied' : 'Copy as Markdown'}</button>
            <button className="btn primary" onClick={close}>Close</button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}
