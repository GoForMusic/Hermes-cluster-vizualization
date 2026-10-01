// The dashboard (Uptime Kuma layout): the list of monitors on the left, the selected one on the right.
import { useMemo, useState } from 'react';
import type { Alert } from '../../../domain/model';
import { fmtCpu, fmtDur, fmtMem, fmtSize } from '../../../domain/format';
import { RANGES } from '../../../domain/hubState';
import type { Node } from '../../../domain/model';
import { PROVIDERS } from '../../../domain/providers';
import { allNodes, clusterOf, clusters as clustersOf, effectiveSeverity, getNode, hiddenSystemCount, isSystem, monitorRows, volumeTotals } from '../../../domain/selectors';
import { STATUS_WORD } from '../../../domain/status';
import { useStore, useWholeState } from '../../../state/context';
import { Icon } from '../../../ui/icons';
import { AlertCard, HeartbeatBar, MiniSymbol, StatTile, StatusChip } from '../../../ui/status';
import { IncidentReport } from '../IncidentReport';
import { ContainerList, NodeDetails } from '../NodeDetails';
import { PageHead } from '../PageHead';
import { useIncidentWindow } from './useIncidentWindow';

const KIND_LABEL = { cluster: 'Cluster', host: 'Host', workload: 'Workload', volume: 'Volume', network: 'Network' } as const;

export function Dashboard({ arg }: { arg: string }) {
  const store = useStore();
  const state = useWholeState();
  const [selected, setSelected] = useState<string | null>(arg && getNode(state, arg) ? arg : null);
  const [filter, setFilter] = useState('');

  const pick = (id: string) => {
    setSelected(id);
    history.replaceState(null, '', `#/admin/dashboard/${id}`); // a deep link, without mounting the page again
  };

  const clusters = clustersOf(state);
  const vols = volumeTotals(state);
  const counts = { ok: 0, warn: 0, crit: 0, unknown: 0 };
  for (const n of allNodes(state)) if (n.kind !== 'cluster' && n.kind !== 'network') counts[n.status]++; // a network has no health of its own
  const total = counts.ok + counts.warn + counts.crit + counts.unknown;
  const hidden = hiddenSystemCount(state);
  const q = filter.trim().toLowerCase();

  // Walking every cluster/host/workload only needs to happen when the topology itself changes, not on every keystroke of the search box.
  const items = useMemo(() => monitorRows(state, clusters), [state]);
  const shown = q ? items.filter((i) => i.level >= 0 && `${i.n.name} ${i.n.meta.ns ?? ''}`.toLowerCase().includes(q)) : items;
  const pct = (id: string) => { const u = state.uptime.get(id); return u ? `${u.pct.toFixed(u.pct === 100 ? 0 : 1)}%` : '—'; };
  const node = selected ? getNode(state, selected) : undefined;

  return (
    <>
      <PageHead title="Dashboard" />
      <div className="grid-tiles">
        <StatTile label="Up" value={counts.ok} sub={`of ${total} monitors`} status="ok" />
        <StatTile label="Degraded" value={counts.warn} sub="needs attention" status={counts.warn ? 'warn' : 'ok'} />
        <StatTile label="Down" value={counts.crit} sub="failing right now" status={counts.crit ? 'crit' : 'ok'} />
        <StatTile label="Unreachable" value={counts.unknown} sub="host is down" status={counts.unknown ? 'unknown' : 'ok'} />
        <StatTile label="Clusters" value={clusters.length} sub="connected sources" />
        {vols.count ? <StatTile label="Storage" value={vols.used.toFixed(1)} unit="GiB" sub={vols.sized ? `of ${vols.size.toFixed(1)} GiB in ${vols.count} ${vols.count === 1 ? 'volume' : 'volumes'}${vols.sized < vols.count ? ' (some with no limit)' : ''}` : `${vols.count} ${vols.count === 1 ? 'volume' : 'volumes'}, no limit set`} /> : null}
      </div>
      <div className="kuma">
        <div className="card mon-list">
          <div className="mon-search">
            <input type="text" placeholder="Search monitors…" value={filter} onChange={(e) => setFilter(e.target.value)} />
            <button className={`pill${state.settings.showSystem ? ' on' : ''}`} title="The cluster's own pods (kube-system, calico-system…). A system pod that is failing is always shown." onClick={() => store.updateSettings((s) => ({ ...s, showSystem: !s.showSystem }))}>
              {state.settings.showSystem ? 'System pods: shown' : `System pods: hidden${hidden ? ` (${hidden})` : ''}`}
            </button>
          </div>
          <div className="mon-rows">
            {shown.length ? shown.map(({ n, level }) => level === -1
              ? <div key={n.id} className="mon-group"><Icon name={PROVIDERS[n.provider].icon} size={14} color={PROVIDERS[n.provider].color} />{n.name}<StatusChip status={n.status}>{STATUS_WORD[n.status]}</StatusChip></div>
              : <MonitorRow key={n.id} node={n} level={q ? 0 : level} selected={selected === n.id} system={isSystem(state, n)} bars={state.uptime.get(n.id)?.bars ?? []} pct={pct(n.id)} onPick={() => pick(n.id)} />)
              : <div className="empty" style={{ margin: 12 }}><b>{clusters.length ? 'Nothing matches' : 'No monitors yet'}</b>{clusters.length ? null : <span>Add a cluster in <a href="#/admin/sources">Sources</a>.</span>}</div>}
          </div>
        </div>
        {node ? <Detail node={node} /> : <div className="card"><div className="empty"><b>Select a monitor</b>Pick a cluster, host, pod or volume on the left.</div></div>}
      </div>
    </>
  );
}

function MonitorRow({ node: n, level, selected, system, bars, pct, onPick }: { node: Node; level: number; selected: boolean; system: boolean; bars: readonly string[]; pct: string; onPick: () => void }) {
  const small = n.kind === 'workload' ? `${n.meta.type} · ${n.meta.ns}` : n.kind === 'host' ? `${n.meta.role} · ${n.meta.ip}${n.meta.osType ? ` · ${n.meta.osType}` : ''}` : KIND_LABEL[n.kind];
  return (
    <div className={`mon-row st-${n.status} l${level}${selected ? ' sel' : ''}`} data-id={n.id} onClick={onPick}>
      <MiniSymbol status={n.status} system={system} />
      <div className="mon-name">{n.name}{system ? <span className="tag-sys">system</span> : null}<small>{small}</small></div>
      <div className="mon-right"><HeartbeatBar bars={bars} small take={24} /><span className="mon-pct">{pct}</span></div>
    </div>
  );
}

function Detail({ node: n }: { node: Node }) {
  const store = useStore();
  const state = useWholeState();
  const u = state.uptime.get(n.id);
  const cl = clusterOf(state, n.id);
  const range = state.range;
  const { picked, pick, clear, pickedWindow, shown, pickedStatus } = useIncidentWindow(n.id, range, u?.bars);
  const [report, setReport] = useState<Alert | null>(null);
  const fmtClock = (ms: number) => new Date(ms).toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  // pods: absolute usage (like kubectl top); hosts: the share of the machine; volumes: how full
  const isPod = n.kind === 'workload';
  const cpu: [string, string] = isPod ? (n.m.cpuMilli == null ? ['—', ''] : [fmtCpu(n.m.cpuMilli), '']) : n.m.cpu == null ? ['—', ''] : [n.m.cpu.toFixed(1), '%'];
  const mem: [string, string] = isPod ? (n.m.memMiB == null ? ['—', ''] : [fmtMem(n.m.memMiB), '']) : n.m.mem == null ? ['—', ''] : [n.m.mem.toFixed(1), '%'];
  const volume = n.kind === 'volume';
  const usage = volume ? (n.meta.size ? `${Math.round(((n.m.used ?? 0) / n.meta.size) * 100)}%` : fmtSize(n.m.used ?? 0)) : cpu[0];
  return (
    <div>
      <div className={`mon-head st-${n.status}`}>
        <div><h2>{n.name}</h2><small>{KIND_LABEL[n.kind]}{cl && cl !== n ? ` · ${cl.name}` : ''}</small></div>
        <span className="pill-status">{STATUS_WORD[n.status]}</span>
      </div>
      <div className="stat-row">
        <StatTile label={`Uptime ${range.id}`} value={u ? u.pct.toFixed(u.pct === 100 ? 0 : 2) : '—'} unit={u ? '%' : ''} status={!u ? 'unknown' : u.pct < 99 ? 'warn' : 'ok'} />
        <StatTile label="Current" value={STATUS_WORD[n.status]} sub={`for ${fmtDur(Date.now() - n.since)}`} status={n.status} />
        <StatTile label={volume ? 'Used' : 'CPU'} value={usage} unit={volume ? '' : cpu[1]} />
        {volume ? null : <StatTile label="Memory" value={mem[0]} unit={mem[1]} />}
      </div>
      <div className="card" style={{ marginBottom: 12 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
          <h3 style={{ margin: 0 }}>{range.label}</h3>
          <div className="tabs">{RANGES.map((r) => <button key={r.id} className={`pill${range.id === r.id ? ' on' : ''}`} onClick={() => store.setRange(r)}>{r.id}</button>)}</div>
        </div>
        <HeartbeatBar bars={u?.bars ?? Array<string>(range.buckets).fill('nodata')} onPick={pick} picked={picked} />
        <div className="muted mono" style={{ display: 'flex', justifyContent: 'space-between', marginTop: 6 }}><span>{range.id} ago</span><span>now</span></div>
        {u?.first ? <div className="muted mono" style={{ marginTop: 4 }}>History recorded since {new Date(u.first).toLocaleString('en-GB')}</div> : null}
      </div>
      <div className="split" style={{ gridTemplateColumns: 'minmax(0,1fr) minmax(0,1fr)' }}>
        <div className="card detail"><h3>Details</h3><NodeDetails state={state} node={n} />{n.kind === 'workload' ? <ContainerList node={n} /> : null}</div>
        <div className="card">
          <h3>
            Events <small className="muted">{pickedWindow ? `${fmtClock(pickedWindow[0])}–${fmtClock(pickedWindow[1])}` : range.label.toLowerCase()}</small>
            {pickedWindow ? <button className="btn xs" style={{ marginLeft: 8 }} onClick={clear}>Show all</button> : null}
          </h3>
          <div className="list">
            {shown.length
              ? shown.map((a) => <AlertCard key={a.id} alert={a} onAck={(id) => void store.ackAlert(id)} onOpen={setReport} context={a.nodeId === n.id ? undefined : `on ${getNode(state, a.nodeId)?.name ?? a.nodeId}`} sev={effectiveSeverity(state, n.id, a)} />)
              : <div className="empty">{pickedStatus ? <><b>No alert recorded</b>Its own status then was “{pickedStatus}” — {pickedStatus === 'warn' ? 'a brief warning never opens a full incident.' : 'nothing crossed the alert threshold.'}</> : `No incidents in the ${range.label.toLowerCase()}`}</div>}
          </div>
        </div>
      </div>
      <IncidentReport alert={report} onClose={() => setReport(null)} onOpen={setReport} />
    </div>
  );
}
