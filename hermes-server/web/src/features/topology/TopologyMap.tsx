// The map: clusters > (networks) > hosts > workloads and volumes, drawn like a tactical map with APP-6 style unit symbols (frame shape and fill are the
// state), a coordinate grid and traffic links. The layout and the link routes depend on the SHAPE of the topology only, so they are computed
// again when it changes and not on every sample; the numbers on it come straight from the state. Orchestration only: the grid, the individual
// box/node/link symbols each live in their own file (`Grid`, `MapHost`, `MapNode`, `MapLink`).
import { useId, useMemo, type MouseEvent } from 'react';
import { CL_HEAD, CL_PAD, MAX_NETWORKS, layoutTopology, moreId, shapeSignature, topologyShape, type ClusterBox, type Layout } from '../../domain/map/layout';
import { computeLinks } from '../../domain/map/links';
import { networkTags } from '../../domain/map/networks';
import { clusterStat, clusterSubtitle, hostStat, nodeTooltip } from '../../domain/mapLabels';
import type { HubState } from '../../domain/hubState';
import { PROVIDERS } from '../../domain/providers';
import { edgeBroken, isDeduced, isSystem, mountLinks, shownKids } from '../../domain/selectors';
import { IconG } from '../../ui/icons';
import { useWholeState } from '../../state/context';
import { usePanZoom, type Inset } from './usePanZoom';
import { Grid } from './Grid';
import { MapHost, HostLabel } from './MapHost';
import { MapNode, MoreNetworks } from './MapNode';
import { MapLink } from './MapLink';

export interface TopologyMapProps {
  /** Limits the clusters drawn; `null` draws every one the settings show. */
  visible?: ReadonlySet<string> | null;
  interactive?: boolean;
  focusId?: string | null;
  inset?: Inset;
  selectedId?: string | null;
  onSelect?: (id: string | null) => void;
  onCursor?: (gridRef: string, zoom: number) => void;
  showLabels?: boolean;
}

export function TopologyMap({ visible = null, interactive = false, focusId = null, inset, selectedId = null, onSelect, onCursor, showLabels = true }: TopologyMapProps) {
  const state = useWholeState();
  const uid = useId();

  const shape = topologyShape(state, visible);
  const sig = shapeSignature(shape);
  // the signature says when the shape changed: the layout is not computed again for a new sample
  const layout = useMemo(() => layoutTopology(shape), [sig]);
  const edges = [...state.edges, ...mountLinks(state)]; // a volume is joined to the workloads that mount it
  const edgeSig = edges.map((e) => `${e.id}${e.type}${e.from}${e.to}`).join('|');
  // routing the links is the expensive part: it is redone only when the shape or the links themselves change
  const links = useMemo(() => new Map(computeLinks(layout, edges).map((l) => [l.edgeId, l])), [layout, edgeSig]);

  const tags = networkTags(state);
  const zoom = usePanZoom({ layout, focusId, interactive, inset, onCursor });

  const related = new Set<string>();
  if (selectedId) {
    related.add(selectedId);
    for (const e of edges) if (e.from === selectedId || e.to === selectedId) { related.add(e.from); related.add(e.to); }
  }
  const select = (id: string) => (e: MouseEvent) => {
    e.stopPropagation();
    if (interactive && !zoom.wasDragged()) onSelect?.(selectedId === id ? null : id);
  };
  const clearSelection = () => { if (interactive && selectedId && !zoom.wasDragged()) onSelect?.(null); };

  const { view } = zoom;
  return (
    <svg
      ref={zoom.svgRef}
      className={`graph${selectedId ? ' has-sel' : ''}${zoom.panning ? ' panning' : ''}${showLabels ? '' : ' no-labels'}`}
      width="100%"
      height="100%"
      onPointerDown={zoom.onPointerDown}
      onPointerMove={onCursor ? zoom.onPointerMove : undefined}
      onClick={clearSelection}
    >
      <g className={`viewport${zoom.animate ? ' anim' : ''}`} style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}>
        <Grid w={layout.w} h={layout.h} />
        <g>
          {layout.clusters.map((c) => (
            <ClusterBoxView key={c.id} box={c} layout={layout} state={state} selectedId={selectedId} interactive={interactive} select={select} />
          ))}
        </g>
        <g>
          {[...tags.colors.keys(), null].map((net) => (
            // the lines of one network share their trunk and bus, so they are made see-through together: drawn one by one, the parts they
            // share would come out darker
            <g key={net ?? 'other'} className={net ? 'net-lines' : undefined}>
              {edges.filter((e) => (net ? e.from === net : !tags.colors.has(e.from))).map((e) => {
                const geometry = links.get(e.id);
                return geometry ? <MapLink key={e.id} pathId={`${uid}-${e.id}`} edge={e} color={tags.colors.get(e.from)} approx={isDeduced(state, e)} geometry={geometry} broken={edgeBroken(state, e)} related={related.has(e.from) && related.has(e.to) && (e.from === selectedId || e.to === selectedId)} /> : null;
              })}
            </g>
          ))}
        </g>
        <g>
          {layout.clusters.flatMap((c) => [...c.networks, ...c.hosts.flatMap((h) => h.items)].map((it) => {
            if (it.id === moreId(c.id)) return <MoreNetworks key={it.id} at={it} count={shownKids(state, c.id).filter((n) => n.kind === 'network').length - (MAX_NETWORKS - 1)} />;
            const n = state.nodes.get(it.id);
            return n ? <MapNode key={it.id} node={n} at={it} system={isSystem(state, n)} selected={selectedId === n.id} related={related.has(n.id) && selectedId !== n.id} tooltip={nodeTooltip(state, n)} onClick={select(n.id)} color={tags.colors.get(n.id)} /> : null;
          }))}
        </g>
        <g>
          {layout.clusters.flatMap((c) => c.hosts.map((h) => {
            const host = state.nodes.get(h.id);
            return host ? <HostLabel key={h.id} box={h} host={host} stat={hostStat(state, host)} selected={selectedId === host.id} onClick={select(h.id)} /> : null;
          }))}
        </g>
      </g>
    </svg>
  );
}

// ------------------------------------------------------------------------------------- clusters and hosts --

interface BoxProps {
  layout: Layout;
  state: HubState;
  selectedId: string | null;
  interactive: boolean;
  select: (id: string) => (e: MouseEvent) => void;
}

function ClusterBoxView({ box, state, selectedId, select }: BoxProps & { box: ClusterBox }) {
  const c = state.nodes.get(box.id);
  if (!c) return null;
  const prov = PROVIDERS[c.provider];
  return (
    <g>
      <rect className={`cluster-box st-${c.status}`} x={box.x} y={box.y} width={box.w} height={box.h} />
      <rect className="cl-band" x={box.x} y={box.y} width={box.w} height={CL_HEAD - 6} />
      <text className={`cl-sum st-${c.status}`} x={box.x + box.w - CL_PAD} y={box.y + 32} textAnchor="end">{clusterStat(state, c)}</text>
      <g className="cl-tile">
        <title>{prov.help}</title>
        <rect x={box.x + CL_PAD} y={box.y + 11} width={36} height={36} fill={prov.color} fillOpacity={0.14} stroke={prov.color} strokeOpacity={0.6} />
        <IconG name={prov.icon} x={box.x + CL_PAD + 7} y={box.y + 18} size={22} color={prov.color} />
      </g>
      <text className="cl-name" x={box.x + CL_PAD + 48} y={box.y + 30}>{c.name}</text>
      <text className="cl-ver" x={box.x + CL_PAD + 48} y={box.y + 46}>{clusterSubtitle(c)}</text>
      {box.hosts.map((h) => {
        const host = state.nodes.get(h.id);
        return host ? <MapHost key={h.id} box={h} host={host} selected={selectedId === host.id} onClick={select(host.id)} /> : null;
      })}
    </g>
  );
}
