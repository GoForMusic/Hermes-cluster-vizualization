// Incident history from the hub's database: filter it (period, severity, cluster, words), open the report of any incident, and export what is
// shown as Markdown for a ticket. Resolved incidents are kept for as many days as Settings says.
import { useMemo, useState } from 'react';
import { incidentsMarkdown, sourceOf } from '../../../domain/incidents';
import type { Alert } from '../../../domain/model';
import { clusters as clustersOf, getNode } from '../../../domain/selectors';
import { useHubState, useStore, useWholeState } from '../../../state/context';
import { AlertCard } from '../../../ui/status';
import { IncidentReport } from '../IncidentReport';
import { PageHead } from '../PageHead';

const PERIODS = [['24h', 86_400_000], ['7d', 7 * 86_400_000], ['30d', 30 * 86_400_000], ['All', 0]] as const;

export function Alerts() {
  const store = useStore();
  const state = useWholeState();
  const alerts = useHubState((s) => s.alerts);
  const [onlyActive, setOnlyActive] = useState(false);
  const [period, setPeriod] = useState<(typeof PERIODS)[number][0]>('7d');
  const [sev, setSev] = useState<'' | 'crit' | 'warn'>('');
  const [cluster, setCluster] = useState('');
  const [text, setText] = useState('');
  const [report, setReport] = useState<Alert | null>(null);
  const [copied, setCopied] = useState(false);

  const clusters = clustersOf(state);
  const rows = useMemo(() => {
    const span = PERIODS.find(([p]) => p === period)![1];
    const since = span ? Date.now() - span : 0;
    const q = text.trim().toLowerCase();
    return alerts.filter((a) =>
      (!onlyActive || a.resolvedTs == null)
      && (a.resolvedTs == null || a.resolvedTs >= since)
      && (!sev || a.sev === sev)
      && (!cluster || sourceOf(a.nodeId) === sourceOf(cluster))
      && (!q || `${a.title} ${a.detail} ${getNode(state, a.nodeId)?.name ?? a.nodeId}`.toLowerCase().includes(q)));
  }, [alerts, onlyActive, period, sev, cluster, text, state]);

  const heading = `Incident report · ${period === 'All' ? 'everything kept' : `last ${period}`}${cluster ? ` · ${clusters.find((c) => c.id === cluster)?.name ?? ''}` : ''}`;
  const exportMd = () => {
    const md = incidentsMarkdown(rows, state, Date.now(), heading);
    void navigator.clipboard?.writeText(md);
    setCopied(true);
    setTimeout(() => setCopied(false), 2500);
  };
  const download = () => {
    const blob = new Blob([incidentsMarkdown(rows, state, Date.now(), heading)], { type: 'text/markdown' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `incidents-${new Date().toISOString().slice(0, 10)}.md`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  return (
    <>
      <PageHead title="Alerts">
        {([[false, 'All'], [true, 'Active']] as const).map(([v, label]) => <button key={label} className={`pill${onlyActive === v ? ' on' : ''}`} onClick={() => setOnlyActive(v)}>{label}</button>)}
      </PageHead>
      <div className="filters">
        <div className="tabs" role="group" aria-label="Period">
          {PERIODS.map(([p]) => <button key={p} className={`pill${period === p ? ' on' : ''}`} onClick={() => setPeriod(p)}>{p}</button>)}
        </div>
        <select aria-label="Severity" value={sev} onChange={(e) => setSev(e.target.value as '' | 'crit' | 'warn')}>
          <option value="">Any severity</option><option value="crit">Critical</option><option value="warn">Warning</option>
        </select>
        <select aria-label="Cluster" value={cluster} onChange={(e) => setCluster(e.target.value)}>
          <option value="">All clusters</option>{clusters.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
        </select>
        <input type="text" placeholder="Search…" aria-label="Search" value={text} onChange={(e) => setText(e.target.value)} />
        <span style={{ flex: 1 }} />
        <span className="muted">{rows.length} incident{rows.length === 1 ? '' : 's'}</span>
        <button className="btn" disabled={!rows.length} onClick={exportMd}>{copied ? 'Copied' : 'Copy as Markdown'}</button>
        <button className="btn" disabled={!rows.length} onClick={download}>Download .md</button>
      </div>
      <div className="list">
        {rows.length
          ? rows.map((a) => <AlertCard key={a.id} alert={a} onAck={(id) => void store.ackAlert(id)} onOpen={setReport} context={(() => { const n = getNode(state, a.nodeId); return n ? n.name : undefined; })()} />)
          : <div className="empty"><b>Nothing here</b>Incidents appear here as they happen and stay after they are resolved.</div>}
      </div>
      <IncidentReport alert={report} onClose={() => setReport(null)} onOpen={setReport} />
    </>
  );
}
