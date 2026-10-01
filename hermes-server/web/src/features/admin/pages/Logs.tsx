// Logs: what the agents and the hub said to each other, to answer "what did agent X send and when", "why is pod Y not there" and "did the hub
// ask for a resync". Off until turned on, kept in the hub's memory only, and never holds a token: it can show pod names, images and addresses.
import { useCallback, useEffect, useState } from 'react';
import type { CommLogEntry } from '../../../generated/CommLogEntry';
import type { CommLogRow } from '../../../generated/CommLogRow';
import type { CommLogView } from '../../../generated/CommLogView';
import { useClient, useHubState } from '../../../state/context';
import { errorText } from '../../../ui/hooks';
import { Toggle } from '../../../ui/toggle';
import { PageHead } from '../PageHead';

const KINDS = [['batch', 'Reports'], ['connected', 'Connected'], ['disconnected', 'Disconnected'], ['resync', 'Resync'], ['upgrade', 'Upgrade'], ['upgrade_status', 'Upgrade status']] as const;
const ARROW: Record<string, string> = { in: '→ hub', out: '← agent', link: '⇄' };

const clock = (ts: number): string => {
  const d = new Date(ts);
  return `${d.toLocaleTimeString('en-GB', { hour12: false })}.${String(d.getMilliseconds()).padStart(3, '0')}`;
};
const size = (bytes: number): string => (bytes ? (bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KiB`) : '');

export function Logs() {
  const client = useClient();
  const sources = useHubState((s) => s.sources);
  const [view, setView] = useState<CommLogView | null>(null);
  const [error, setError] = useState('');
  const [source, setSource] = useState('');
  const [kind, setKind] = useState('');
  const [text, setText] = useState('');
  const [follow, setFollow] = useState(true);
  const [open, setOpen] = useState<number | null>(null);
  const [entry, setEntry] = useState<CommLogEntry | null>(null);
  const [gone, setGone] = useState(false);
  const [copied, setCopied] = useState(false);

  const load = useCallback(async () => {
    try { setView(await client.commLog.list({ source, kind, q: text, limit: 300 })); setError(''); } catch (e) { setError(errorText(e)); }
  }, [client, source, kind, text]);

  useEffect(() => { void load(); }, [load]);
  useEffect(() => {
    if (!follow || !view?.enabled) return;
    const t = setInterval(() => void load(), 2000);
    return () => clearInterval(t);
  }, [follow, view?.enabled, load]);

  useEffect(() => {
    setEntry(null);
    setGone(false);
    if (open == null) return;
    let live = true;
    void client.commLog.get(open).then((e) => { if (live) setEntry(e); }).catch(() => { if (live) setGone(true); });
    return () => { live = false; };
  }, [client, open]);

  const switchTo = async (on: boolean) => {
    try { await client.commLog.enable(on); if (!on) setOpen(null); await load(); } catch (e) { setError(errorText(e)); }
  };
  const clear = async () => { try { await client.commLog.clear(); setOpen(null); await load(); } catch (e) { setError(errorText(e)); } };

  const rows: CommLogRow[] = view?.entries ?? [];
  const json = entry ? JSON.stringify(entry.body, null, 2) : '';
  const download = () => {
    const blob = new Blob([JSON.stringify(rows, null, 2)], { type: 'application/json' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `agent-log-${new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  return (
    <>
      <PageHead title="Logs" />
      <div className="card" style={{ marginBottom: 12 }}>
        <div className="row" style={{ border: 0, padding: 0 }}>
          <div>
            <b>Record the agents’ traffic</b>
            <span className="d">
              Off by default. While it is on, the hub keeps the last {view?.capacity ?? 500} messages, or {view?.minutes ?? 15} minutes, in memory: what each agent reported and what the hub asked of it. It can show pod names, images and addresses, never tokens, and only you see it. Turning it off, or restarting the hub, forgets it.
            </span>
          </div>
          <Toggle checked={view?.enabled ?? false} label="Record the agents’ traffic" onChange={(on) => void switchTo(on)} />
        </div>
      </div>
      {error ? <div className="err" role="alert">{error}</div> : null}
      {view?.enabled ? (
        <>
          <div className="filters">
            <select aria-label="Source" value={source} onChange={(e) => setSource(e.target.value)}>
              <option value="">All sources</option>{sources.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
            </select>
            <select aria-label="Kind" value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="">Everything</option>{KINDS.map(([k, label]) => <option key={k} value={k}>{label}</option>)}
            </select>
            <input type="text" placeholder="Search names, images, reasons…" aria-label="Search" value={text} onChange={(e) => setText(e.target.value)} />
            <label className="check" style={{ display: 'inline-flex', gap: 6, alignItems: 'center' }}><input type="checkbox" checked={follow} onChange={(e) => setFollow(e.target.checked)} />Follow live</label>
            <span style={{ flex: 1 }} />
            <span className="muted" title="Empty reports, one a second from each agent: counted, not listed">{rows.length} shown · {view.heartbeats.toLocaleString('en-GB')} heartbeats</span>
            <button className="btn" disabled={!rows.length} onClick={download}>Download</button>
            <button className="btn danger" disabled={!rows.length} onClick={() => void clear()}>Clear</button>
          </div>
          <div className="split" style={{ gridTemplateColumns: open == null ? 'minmax(0,1fr)' : 'minmax(0,1.2fr) minmax(0,1fr)' }}>
            <div className="card" style={{ padding: 0, overflow: 'auto', maxHeight: '70vh' }}>
              {rows.length ? (
                <table className="log-table">
                  <thead><tr><th>Time</th><th>Source</th><th>Agent</th><th /><th>What</th><th>Size</th></tr></thead>
                  <tbody>
                    {rows.map((r) => (
                      <tr key={r.id} className={open === r.id ? 'on' : ''} onClick={() => setOpen(open === r.id ? null : r.id)} tabIndex={0} onKeyDown={(e) => { if (e.key === 'Enter') setOpen(r.id); }}>
                        <td className="mono">{clock(r.ts)}</td>
                        <td>{r.sourceName}</td>
                        <td className="mono">{r.agent}</td>
                        <td className="muted">{ARROW[r.dir] ?? r.dir}</td>
                        <td><span className={`kind k-${r.kind}`}>{r.kind.replace('_', ' ')}</span> {r.summary}</td>
                        <td className="muted mono">{size(r.bytes)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : <div className="empty"><b>Nothing yet</b>{text || source || kind ? 'Nothing matches these filters.' : 'Waiting for the agents to say something new; their empty heartbeats are only counted.'}</div>}
            </div>
            {open != null ? (
              <div className="card">
                <div className="src-top"><h3 style={{ margin: 0 }}>{entry ? `${entry.kind.replace('_', ' ')} · ${entry.agent}` : 'Entry'}</h3><button className="btn xs" onClick={() => setOpen(null)}>Close</button></div>
                {gone ? <div className="empty">That entry is gone: the log keeps the last {view.capacity}, or {view.minutes} minutes.</div> : null}
                {entry ? (
                  <>
                    <p className="muted" style={{ fontSize: 12 }}>{new Date(entry.ts).toLocaleString('en-GB', { hour12: false })} · {entry.sourceName} · {ARROW[entry.dir]} · {size(entry.bytes) || 'no size'}</p>
                    <pre className="code" style={{ maxHeight: '55vh', overflow: 'auto' }}>{json}</pre>
                    <div className="dlg-actions" style={{ justifyContent: 'flex-start' }}>
                      <button className="btn" onClick={() => { void navigator.clipboard?.writeText(json); setCopied(true); setTimeout(() => setCopied(false), 2000); }}>{copied ? 'Copied' : 'Copy JSON'}</button>
                    </div>
                  </>
                ) : null}
              </div>
            ) : null}
          </div>
        </>
      ) : <div className="card"><div className="empty"><b>The log is off</b>Turn it on above to see what the agents send and what the hub asks of them.</div></div>}
    </>
  );
}
