// The map: clusters > (networks) > hosts > workloads and volumes, drawn like a tactical map with APP-6 style unit symbols (frame shape and fill are the
// state), a coordinate grid and traffic links. The layout and the link routes depend on the SHAPE of the topology only, so they are computed
// again when it changes and not on every sample; the numbers on it come straight from the state. Orchestration only: the grid, the individual
// box/node/link symbols each live in their own file (`Grid`, `MapHost`, `MapNode`, `MapLink`).
import { useEffect, useId, useMemo, useRef, useState, type CSSProperties, type MouseEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { CL_HEAD, CL_PAD, MAX_NETWORKS, layoutGrouped, layoutToFit, moreId, shapeSignature, topologyShape, type ClusterBox, type Layout, type RegionBox } from '../../domain/map/layout';
import { computeLinks } from '../../domain/map/links';
import { networkTags } from '../../domain/map/networks';
import { clusterStat, clusterSubtitle, hostStat, nodeTooltip } from '../../domain/mapLabels';
import type { HubState } from '../../domain/hubState';
import { PROVIDERS } from '../../domain/providers';
import { edgeBroken, isDeduced, isSystem, mountLinks, shownKids } from '../../domain/selectors';
import { IconG } from '../../ui/icons';
import { useWholeState } from '../../state/context';
import { usePanZoom, type Inset } from './usePanZoom';
import { useHostCols } from './useHostCols';
import { useClusterRows } from './useClusterRows';
import { regionOf } from '../../domain/regions';
import { arrange, dropSide, rowsOf, type Side } from '../../domain/map/order';
import { Grid } from './Grid';
import { MapHost, HostLabel, HostGrip } from './MapHost';
import { MapNode, MoreNetworks } from './MapNode';
import { MapLink } from './MapLink';

export interface TopologyMapProps {
  /** Limits the clusters drawn; `null` draws every one the settings show. */
  visible?: ReadonlySet<string> | null;
  interactive?: boolean;
  focusId?: string | null;
  inset?: Inset;
  maxFit?: number;
  selectedId?: string | null;
  onSelect?: (id: string | null) => void;
  onCursor?: (gridRef: string, zoom: number) => void;
  showLabels?: boolean;
  /** Right click on a cluster or a region frame (admin only): where the screen was clicked. */
  onMenu?: (target: MapTarget, at: { x: number; y: number }) => void;
  /** Admin only: a cluster was dropped in a region (`null`: outside all of them). */
  onAssign?: (clusterId: string, regionId: string | null) => void;
}

export interface MapTarget { kind: 'cluster' | 'region'; id: string }

export function TopologyMap({ visible = null, interactive = false, focusId = null, inset, maxFit, selectedId = null, onSelect, onCursor, showLabels = true, onMenu, onAssign }: TopologyMapProps) {
  const state = useWholeState();
  const uid = useId();

  const arrangement = useClusterRows(); // the lines a person gave the clusters by dragging them, else the screen decides
  const shape = topologyShape(state, visible);
  const sig = shapeSignature(shape);
  // the signature says when the shape changed: the layout is not computed again for a new sample
  const hostCols = useHostCols();
  const colsSig = [...hostCols.cols].map(([id, n]) => `${id}=${n}`).join(',');
  // the size of the screen decides how the clusters are arranged: rows as wide as the screen, not a fixed width
  const svgRef = useRef<SVGSVGElement>(null);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);
  useEffect(() => {
    const el = svgRef.current?.parentElement;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = el.getBoundingClientRect();
      const w = Math.round(r.width / 50) * 50, h = Math.round(r.height / 50) * 50; // not a new layout for every pixel of a resize
      setSize((prev) => (prev && prev.w === w && prev.h === h ? prev : { w, h }));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const room = size && { w: size.w, h: size.h - (inset?.top ?? 0) - (inset?.bottom ?? 0) };
  const rowsSig = arrangement.rows ? JSON.stringify(arrangement.rows) : '';
  const regions = state.settings.regions;
  const regionsSig = JSON.stringify(regions);
  const layout = useMemo(() => layoutToFit(shape, hostCols.cols, room, arrangement.rows ?? undefined, regions), [sig, colsSig, rowsSig, regionsSig, room?.w, room?.h]);
  const edges = [...state.edges, ...mountLinks(state)]; // a volume is joined to the workloads that mount it
  const edgeSig = edges.map((e) => `${e.id}${e.type}${e.from}${e.to}`).join('|');
  // routing the links is the expensive part: it is redone only when the shape or the links themselves change
  const links = useMemo(() => new Map(computeLinks(layout, edges).map((l) => [l.edgeId, l])), [layout, edgeSig]);

  const tags = networkTags(state);
  const zoom = usePanZoom({ layout, focusId, interactive, inset, svgRef, maxFit, onCursor });

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

  // Dragging a cluster by its title band onto another: on its left or right it joins that line, above or below it gets a line of its own.
  // While it happens the map stays where it is and a sheet shows where every cluster would be (the one moved is the shadow box).
  const [drag, setDrag] = useState<{ id: string; over: string | null; side: Side; region: string | null | undefined } | null>(null);
  const previewRows = drag?.over ? arrange(rowsOf(layout.clusters), drag.id, drag.over, drag.side) : null;
  const previewKey = previewRows ? JSON.stringify(previewRows) : '';
  const preview = useMemo(() => (previewRows ? layoutGrouped(shape, hostCols.cols, 1500, previewRows, regions) : null), [previewKey, sig, colsSig, regionsSig]);
  const grab = (id: string) => (e: ReactPointerEvent<SVGElement>) => {
    if (!interactive || e.button !== 0) return;
    e.stopPropagation(); // not a pan of the map
    zoom.hold();
    const svg = zoom.svgRef.current;
    if (!svg) return;
    const rect = svg.getBoundingClientRect(), v = view, x0 = e.clientX, y0 = e.clientY;
    let moved = false, over: string | null = null, side: Side = 'l', region: string | null | undefined;
    const at = (ev: PointerEvent) => {
      const px = (ev.clientX - rect.left - v.x) / v.k, py = (ev.clientY - rect.top - v.y) / v.k;
      const inside = layout.clusters.find((c) => px >= c.x && px <= c.x + c.w && py >= c.y && py <= c.y + c.h);
      const frame = layout.regions.find((r) => px >= r.x && px <= r.x + r.w && py >= r.y && py <= r.y + r.h);
      // dropped on a cluster it joins that cluster's region; on the ground of a region, that region; anywhere else, none
      const to = inside && inside.id !== id ? regionOf(regions, inside.id) : frame ? frame.id : null;
      return { hit: inside && inside.id !== id ? { id: inside.id, side: dropSide(inside, px, py) } : null, to };
    };
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return; // a click, not a drag
      moved = true;
      const { hit, to } = at(ev);
      over = hit?.id ?? null;
      side = hit?.side ?? 'l';
      region = onAssign ? to : undefined;
      setDrag({ id, over, side, region });
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      if (moved && over) arrangement.set(arrange(rowsOf(layout.clusters), id, over, side));
      if (moved && region !== undefined && region !== regionOf(regions, id)) onAssign?.(id, region);
      if (moved && (over || region !== undefined)) zoom.release(); // the new arrangement is fitted to the screen again, not left partly out of it
      setDrag(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };
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
        <g>{layout.regions.map((r) => <RegionFrame key={r.id} box={r} onMenu={onMenu} drop={drag?.region === r.id} />)}</g>
        <g>
          {layout.clusters.map((c) => (
            <ClusterBoxView key={c.id} box={c} layout={layout} state={state} selectedId={selectedId} interactive={interactive} select={select} onGrab={interactive ? grab(c.id) : undefined} onReset={interactive ? arrangement.clear : undefined} onMenu={onMenu} mark={drag?.id === c.id ? 'drag' : undefined} />
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
        {preview && drag ? <ArrangePreview layout={preview} movingId={drag.id} state={state} /> : null}
        {interactive ? (
          <g>
            {layout.clusters.flatMap((c) => c.hosts.map((h) => <HostGrip key={h.id} box={h} scale={view.k} onHold={zoom.hold} onCols={(n) => hostCols.set(h.id, n)} />))}
          </g>
        ) : null}
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

interface ClusterDrag {
  /** Pointer down on the title band: starts moving the cluster. */
  onGrab?: (e: ReactPointerEvent<SVGElement>) => void;
  /** Double click on the band: back to the automatic arrangement. */
  onReset?: () => void;
  /** `drag`: this one is being moved. */
  mark?: 'drag';
  onMenu?: TopologyMapProps['onMenu'];
}

function ClusterBoxView({ box, state, selectedId, select, onGrab, onReset, mark, onMenu }: BoxProps & ClusterDrag & { box: ClusterBox }) {
  const c = state.nodes.get(box.id);
  if (!c) return null;
  const prov = PROVIDERS[c.provider];
  return (
    <g className={mark ? `cl-${mark}` : undefined}>
      <rect className={`cluster-box st-${c.status}`} x={box.x} y={box.y} width={box.w} height={box.h} />
      {/* the whole header grabs: the texts and the icon sit over the band and would take the pointer themselves, and the map would move */}
      <g className={onGrab ? 'cl-head grab' : 'cl-head'} onPointerDown={onGrab} onContextMenu={onMenu ? (e) => { e.preventDefault(); e.stopPropagation(); onMenu({ kind: 'cluster', id: box.id }, { x: e.clientX, y: e.clientY }); } : undefined} onDoubleClick={onReset ? (e) => { e.stopPropagation(); onReset(); } : undefined}>
        <rect className="cl-band" x={box.x} y={box.y} width={box.w} height={CL_HEAD - 6}>
          {onGrab ? <title>Drag onto another cluster to move it: left or right of it = same line, above or below = a line of its own. Drop it in a region to put it there. Double click: automatic.</title> : null}
        </rect>
        <text className={`cl-sum st-${c.status}`} x={box.x + box.w - CL_PAD} y={box.y + 32} textAnchor="end">{clusterStat(state, c)}</text>
        <g className="cl-tile">
          <title>{prov.help}</title>
          <rect x={box.x + CL_PAD} y={box.y + 11} width={36} height={36} fill={prov.color} fillOpacity={0.14} stroke={prov.color} strokeOpacity={0.6} />
          <IconG name={prov.icon} x={box.x + CL_PAD + 7} y={box.y + 18} size={22} color={prov.color} />
        </g>
        <text className="cl-name" x={box.x + CL_PAD + 48} y={box.y + 30}>{c.name}</text>
        <text className="cl-ver" x={box.x + CL_PAD + 48} y={box.y + 46}>{clusterSubtitle(c)}</text>
      </g>
      {box.hosts.map((h) => {
        const host = state.nodes.get(h.id);
        return host ? <MapHost key={h.id} box={h} host={host} selected={selectedId === host.id} onClick={select(host.id)} /> : null;
      })}
    </g>
  );
}

/** The frame of a region: under everything else, so it only tints the ground its clusters stand on. */
function RegionFrame({ box, onMenu, drop }: { box: RegionBox; onMenu?: TopologyMapProps['onMenu']; drop: boolean }) {
  const open = onMenu ? (e: MouseEvent) => { e.preventDefault(); e.stopPropagation(); onMenu({ kind: 'region', id: box.id }, { x: e.clientX, y: e.clientY }); } : undefined;
  return (
    <g className={drop ? 'region drop' : 'region'} style={{ '--rg': box.color } as CSSProperties} onContextMenu={open}>
      <rect className="region-box" x={box.x} y={box.y} width={box.w} height={box.h} />
      <text className="region-name" x={box.x + 18} y={box.y + 28}>{box.name}</text>
    </g>
  );
}

/** While a cluster is dragged: where every cluster would be if it were dropped now. The one being moved is the shadow box. */
function ArrangePreview({ layout, movingId, state }: { layout: Layout; movingId: string; state: HubState }) {
  return (
    <g className="arrange" pointerEvents="none">
      <rect className="arrange-sheet" x={-80} y={-80} width={layout.w + 160} height={layout.h + 160} />
      {layout.clusters.map((c) => (
        <g key={c.id} className={c.id === movingId ? 'arrange-box moving' : 'arrange-box'}>
          <rect x={c.x} y={c.y} width={c.w} height={c.h} />
          <text x={c.x + 20} y={c.y + 36}>{state.nodes.get(c.id)?.name ?? c.id}</text>
        </g>
      ))}
    </g>
  );
}
