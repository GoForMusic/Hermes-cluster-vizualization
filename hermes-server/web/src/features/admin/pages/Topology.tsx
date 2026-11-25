// The map with pan and zoom, a filter by provider, and a side panel for the selected node.
import { useState } from 'react';
import { can } from '../../../domain/access';
import { useSession } from '../../../app/session';
import { LogDock, type LogTarget } from '../../topology/LogDock';
import { fmtMbps } from '../../../domain/format';
import { isGateway } from '../../../domain/mapLabels';
import type { ProviderId } from '../../../domain/model';
import { PROVIDERS } from '../../../domain/providers';
import { activeAlerts, clusterOf, clusters, getNode, hiddenSystemCount, series } from '../../../domain/selectors';
import { useStore, useWholeState } from '../../../state/context';
import { Icon } from '../../../ui/icons';
import { AlertCard } from '../../../ui/status';
import { Sparkline } from '../../../ui/sparkline';
import { Toggle } from '../../../ui/toggle';
import { useAdminShell } from '../adminContext';
import { ContainerList, NodeDetails, helpFor } from '../NodeDetails';
import { PageHead } from '../PageHead';
import { Legend } from '../../topology/Legend';
import { TopologyMap, type MapTarget } from '../../topology/TopologyMap';
import { assign, withRegions } from '../../../domain/regions';
import { RegionMenu } from '../../topology/RegionMenu';

const LEGEND_SEEN = 'hermes.legendSeen';

/** The legend opens by itself the first time, on a screen wide enough not to cover the map. */
function firstVisit(): boolean {
  try {
    const seen = localStorage.getItem(LEGEND_SEEN) != null;
    localStorage.setItem(LEGEND_SEEN, '1');
    return !seen && window.innerWidth > 1100;
  } catch { return false; /* storage is not available */ }
}

export function Topology({ arg }: { arg: string }) {
  const store = useStore();
  const state = useWholeState();
  const { setCursor } = useAdminShell();
  const [filter, setFilter] = useState<'all' | ProviderId>('all');
  const [selected, setSelected] = useState<string | null>(arg && getNode(state, arg) ? arg : null); // a deep link: #/admin/topology/<nodeId>
  const [labels, setLabels] = useState(true);
  const [legendOpen, setLegendOpen] = useState(firstVisit);
  const [log, setLog] = useState<LogTarget | null>(null);
  const [menu, setMenu] = useState<{ target: MapTarget; at: { x: number; y: number } } | null>(null);

  const all = clusters(state);
  const providers = [...new Set(all.map((c) => c.provider))];
  const visible = filter === 'all' ? null : new Set(all.filter((c) => c.provider === filter).map((c) => c.id));
  const hidden = hiddenSystemCount(state);
  const choose = (f: 'all' | ProviderId) => { setFilter(f); setSelected(null); };

  return (
    <>
      <PageHead title="Topology"><span className="muted">wheel = zoom · middle button = move the view · left drag = move a cluster (into a region too) or resize a host · drag a region by its name = move it · right click = regions · double-click = fit</span></PageHead>
      <div className="graph-toolbar">
        <div className="tabs">
          {(['all', ...providers] as const).map((p) => (
            <button key={p} className={`pill${filter === p ? ' on' : ''}`} onClick={() => choose(p)}>
              {p === 'all' ? 'All' : <><Icon name={PROVIDERS[p].icon} size={15} color={PROVIDERS[p].color} />{PROVIDERS[p].label}</>}
            </button>
          ))}
        </div>
        <button className={`pill${state.settings.showSystem ? ' on' : ''}`} title="The cluster's own pods (kube-system, calico-system…). A system pod that is failing is always drawn." onClick={() => store.updateSettings((s) => ({ ...s, showSystem: !s.showSystem }))}>
          {state.settings.showSystem ? 'System pods: shown' : `System pods: hidden${hidden ? ` (${hidden})` : ''}`}
        </button>
        <span style={{ flex: 1 }} />
        <span className="muted">Traffic labels</span>
        <Toggle checked={labels} onChange={setLabels} label="Traffic labels" />
      </div>
      <div className="split">
        <div className="card graph-card tall">
          <TopologyMap visible={visible} interactive selectedId={selected} onSelect={setSelected} showLabels={labels} onAssign={(id, region) => store.updateSettings((s) => withRegions(s, assign(s.regions, id, region)))} onRegionRows={(rows) => store.updateSettings((s) => ({ ...s, regionRows: rows }))} onMenu={(target, at) => setMenu({ target, at })} onCursor={(ref, k) => setCursor(`Grid ${ref} · Zoom ${Math.round(k * 100)}%`)} />
          <Legend open={legendOpen} onOpenChange={setLegendOpen} />
          {menu ? <RegionMenu key={`${menu.target.kind}${menu.target.id}${menu.at.x}`} target={menu.target} at={menu.at} onClose={() => setMenu(null)} /> : null}
        </div>
        <Panel selected={selected} onLogs={setLog} />
      </div>
      {log ? <LogDock key={`${log.source}/${log.namespace}/${log.name}`} target={log} onClose={() => setLog(null)} /> : null}
    </>
  );
}

function Panel({ selected, onLogs }: { selected: string | null; onLogs: (t: LogTarget) => void }) {
  const state = useWholeState();
  const n = getNode(state, selected);
  if (!n) return <div className="card detail"><h3>Details</h3><div className="empty">{state.nodes.size ? 'Click a host, pod or volume in the map.' : 'Nothing to show yet. Add a source first.'}</div></div>;
  const links = state.edges.filter((e) => (e.from === n.id || e.to === n.id) && e.type === 'traffic');
  const routes = state.edges.filter((e) => (e.from === n.id || e.to === n.id) && e.type === 'route');
  const alerts = activeAlerts(state).filter((a) => a.nodeId === n.id);
  return (
    <div className="card detail">
      <h3>Details</h3>
      <h2>{n.name}</h2>
      <p className="help">{helpFor(n)}</p>
      <NodeDetails state={state} node={n} />
      {n.kind !== 'volume' && n.kind !== 'cluster' && n.kind !== 'network' ? <div className="sec"><h4>CPU · last minute</h4><Sparkline values={series(state, `cpu:${n.id}`)} width={300} height={40} /></div> : null}
      {n.kind === 'workload' ? <ContainerList node={n} /> : null}
      {n.kind === 'workload' ? <LogsButton node={n} onLogs={onLogs} /> : null}
      {links.length ? (
        <div className="sec">
          <h4>Traffic</h4>
          {links.map((e) => <div key={e.id} className="link-row"><span>{e.from === n.id ? '→' : '←'} {getNode(state, e.from === n.id ? e.to : e.from)?.name ?? '?'}</span><b>{fmtMbps(e.mbps)}</b></div>)}
        </div>
      ) : null}
      {routes.length ? (
        <div className="sec">
          <h4>{n.kind === 'network' ? (n.meta.netKind === 'policy' ? 'Applies to' : isGateway(n) ? 'Leads to' : 'On it') : 'Reached through'}</h4>
          {routes.map((e) => <div key={e.id} className="link-row"><span>{e.from === n.id ? '→' : '←'} {getNode(state, e.from === n.id ? e.to : e.from)?.name ?? '?'}</span></div>)}
        </div>
      ) : null}
      {alerts.length ? <div className="sec"><h4>Active alerts</h4><div className="list">{alerts.map((a) => <AlertCard key={a.id} alert={a} />)}</div></div> : null}
    </div>
  );
}

/** The way into the log panel. Only offered to who may read that log; when the source was not installed to allow it, it says how to. */
function LogsButton({ node: n, onLogs }: { node: NonNullable<ReturnType<typeof getNode>>; onLogs: (t: LogTarget) => void }) {
  const state = useWholeState();
  const { auth } = useSession();
  const cluster = clusterOf(state, n.id);
  const source = state.sources.find((s) => s.id === cluster?.id);
  if (!cluster || !source) return null;
  const namespace = String(n.meta.ns || '-');
  if (!can(auth.grants, 'read', `logs/${cluster.id}/${namespace}/${n.name}`)) return null;
  if (source.dc) return <div className="sec"><h4>Log</h4><p className="muted" style={{ margin: 0 }}>This cluster belongs to the datacenter {source.dcName}: read its logs on that hub.</p></div>;
  const containers = cluster.provider === 'kubernetes' ? (n.meta.containers ?? []).map((c) => c.name) : [];
  return (
    <div className="sec">
      <h4>Log</h4>
      {source.canLogs ? (
        <button className="btn primary xs" onClick={() => onLogs({ source: cluster.id, namespace, name: n.name, task: n.meta.taskID, containers })}>Open the log</button>
      ) : (
        <p className="muted" style={{ margin: 0 }}>Reading logs is off for this source. Add it again with “Allow reading container logs” ticked and apply that manifest once.</p>
      )}
    </div>
  );
}
