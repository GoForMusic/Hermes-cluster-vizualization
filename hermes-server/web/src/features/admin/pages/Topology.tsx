// The map with pan and zoom, a filter by provider, and a side panel for the selected node.
import { useState } from 'react';
import { fmtMbps } from '../../../domain/format';
import { isGateway } from '../../../domain/mapLabels';
import type { ProviderId } from '../../../domain/model';
import { PROVIDERS } from '../../../domain/providers';
import { activeAlerts, clusters, getNode, hiddenSystemCount, series } from '../../../domain/selectors';
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
import { assign } from '../../../domain/regions';
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
          <TopologyMap visible={visible} interactive selectedId={selected} onSelect={setSelected} showLabels={labels} onAssign={(id, region) => store.updateSettings((s) => ({ ...s, regions: assign(s.regions, id, region) }))} onRegionRows={(rows) => store.updateSettings((s) => ({ ...s, regionRows: rows }))} onMenu={(target, at) => setMenu({ target, at })} onCursor={(ref, k) => setCursor(`Grid ${ref} · Zoom ${Math.round(k * 100)}%`)} />
          <Legend open={legendOpen} onOpenChange={setLegendOpen} />
          {menu ? <RegionMenu key={`${menu.target.kind}${menu.target.id}${menu.at.x}`} target={menu.target} at={menu.at} onClose={() => setMenu(null)} /> : null}
        </div>
        <Panel selected={selected} />
      </div>
    </>
  );
}

function Panel({ selected }: { selected: string | null }) {
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
