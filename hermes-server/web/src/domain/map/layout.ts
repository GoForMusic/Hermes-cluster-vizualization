// Where everything goes on the map: clusters > (a row of networks and their bus lines) > hosts > workloads and volumes. Pure geometry: it works on the SHAPE of the topology (which
// ids sit where), never on live numbers, so it is computed again only when the shape changes, not on every metrics sample.
import { isShown, kidsOf, shownKids, visibleClusters } from '../selectors';
import type { Region } from '../settings';
import type { HubState } from '../hubState';

export const CELL_W = 112, CELL_H = 104;
export const HOST_PAD = 14, HOST_HEAD = 64, HOST_GAP = 22;
export const CL_PAD = 18, CL_HEAD = 56, CL_GAP = 46;
const MAX_ROW_W = 1500;
const CTRL_LANE = 50; // space under the hosts for control-plane links
/** Under the row of network plates, each network has a bus line (BUS_GAP apart) from which its branches go down to what is on it. */
export const BUS_GAP = 8, BUS_PAD = 6;

/** A cluster draws at most this many networks; the rest are one plate that says how many there are (a wallboard cannot show fifty). */
export const MAX_NETWORKS = 12;
export const moreId = (clusterId: string): string => `more:${clusterId}`;

export interface ShapeItem { id: string; volume: boolean }
export interface ShapeHost { id: string; items: ShapeItem[] }
/** `networks` are the ids of the networks of the cluster (they belong to no host: a network reaches across them). */
export interface ShapeCluster { id: string; networks: string[]; hosts: ShapeHost[]; hasControl: boolean }

/** The networks drawn: all of them up to the cap, then the first ones and a plate for the rest. */
const capNetworks = (clusterId: string, ids: string[]): string[] => (ids.length > MAX_NETWORKS ? [...ids.slice(0, MAX_NETWORKS - 1), moreId(clusterId)] : ids);

/** Which clusters, hosts and pods are drawn, in what order. `visible` limits the clusters (null: all the ones the settings show). */
export function topologyShape(s: HubState, visible: ReadonlySet<string> | null): ShapeCluster[] {
  return visibleClusters(s)
    .filter((c) => !visible || visible.has(c.id))
    .map((c) => {
      const hosts = kidsOf(s, c.id).filter((n) => n.kind === 'host');
      return {
        id: c.id,
        networks: capNetworks(c.id, shownKids(s, c.id).filter((n) => n.kind === 'network').map((n) => n.id)),
        hosts: hosts.map((h) => ({
          id: h.id,
          items: shownKids(s, h.id)
            .filter((n) => n.kind === 'workload' || n.kind === 'volume')
            .map((n) => ({ id: n.id, volume: n.kind === 'volume' }))
            .sort((a, b) => Number(a.volume) - Number(b.volume)),
        })),
        hasControl: s.edges.some((e) => e.type === 'control' && hosts.some((h) => h.id === e.from)),
      };
    });
}

/** A short string that changes when the shape does: what a memo keys on. */
export const shapeSignature = (shape: readonly ShapeCluster[]): string =>
  shape.map((c) => `${c.id}${c.hasControl ? '*' : ''}<${c.networks.join(',')}>[${c.hosts.map((h) => `${h.id}(${h.items.map((i) => i.id).join(',')})`).join(';')}]`).join('|');

export interface Placed { id: string; x: number; y: number }
/** A network plate. `trunk` is the x of the line that leaves it down to its bus, `bus` the y of that bus. */
export interface NetworkPlaced extends Placed { trunk: number; bus: number }
/** `cols` is how many columns its workloads are laid out in: what the person's resize changes. */
export interface HostBox { id: string; x: number; y: number; w: number; h: number; cols: number; items: Placed[] }
export interface ClusterBox { id: string; x: number; y: number; w: number; h: number; hosts: HostBox[]; networks: NetworkPlaced[] }
/** The frame of a region, in map coordinates. */
export interface RegionBox { id: string; name: string; color: string; x: number; y: number; w: number; h: number }
export interface Layout {
  clusters: ClusterBox[];
  /** Frames drawn under the clusters; empty without regions. */
  regions: RegionBox[];
  /** the centre of every workload, volume and network */
  items: Map<string, Placed>;
  hosts: Map<string, HostBox>;
  w: number;
  h: number;
}

/** Which clusters sit on the same line, top to bottom and left to right: what a person arranged by dragging. */
export type Rows = readonly (readonly string[])[];

/** The most columns a host can be stretched to. */
export const MAX_COLS = 8;
/** How many columns a host of `n` items gets by itself. */
export const autoCols = (n: number): number => (n <= 2 ? Math.max(n, 1) : n <= 4 ? 2 : 3);

/** `colsOf` holds the columns a person chose for some hosts (by id); the others are laid out by `autoCols`. */
export function layoutTopology(shape: readonly ShapeCluster[], colsOf: ReadonlyMap<string, number> = new Map(), maxRowW: number = MAX_ROW_W, rows?: Rows): Layout {
  interface Prepared { host: ShapeHost; cols: number; rows: number; w: number }
  const boxes = shape.map((c) => {
    const prepared: Prepared[] = c.hosts.map((host) => {
      const n = host.items.length;
      const chosen = colsOf.get(host.id);
      const cols = chosen ? Math.min(Math.max(Math.round(chosen), 1), Math.min(MAX_COLS, Math.max(n, 1))) : autoCols(n);
      return { host, cols, rows: Math.max(1, Math.ceil(n / cols)), w: Math.max(244, cols * CELL_W + 2 * HOST_PAD) };
    });
    // a cluster that reported nothing (its agent is gone) has no hosts to draw: just its header
    const hostH = prepared.length ? HOST_HEAD + Math.max(...prepared.map((p) => p.rows)) * CELL_H + HOST_PAD - 6 : 6;
    const hostsW = prepared.reduce((sum, p) => sum + p.w + HOST_GAP, 0) - HOST_GAP + 2 * CL_PAD;
    // the networks sit in a row above the hosts (wrapped to the width of the cluster), and under it a bus line for each
    const nets = c.networks.length;
    const w = Math.max(350, hostsW, Math.min(nets, 6) * CELL_W + 2 * CL_PAD);
    const perRow = Math.max(1, Math.floor((w - 2 * CL_PAD) / CELL_W));
    const rows = Math.ceil(nets / perRow);
    const busTop = CL_HEAD + rows * CELL_H + BUS_PAD;
    const bandH = nets ? rows * CELL_H + 2 * BUS_PAD + nets * BUS_GAP : 0;
    const networks: NetworkPlaced[] = c.networks.map((id, i) => {
      const row = Math.floor(i / perRow), inRow = Math.min(perRow, nets - row * perRow);
      const x = (w - inRow * CELL_W) / 2 + (i % perRow) * CELL_W + CELL_W / 2;
      // the trunk runs down the free corridor to the right of the plate; plates in different rows take a slightly different one
      return { id, x, y: CL_HEAD + row * CELL_H + 34, trunk: x + CELL_W / 2 + row * 4, bus: busTop + i * BUS_GAP };
    });
    let x = CL_PAD;
    const hosts: HostBox[] = prepared.map((p) => {
      const ox = (p.w - p.cols * CELL_W) / 2;
      const box: HostBox = {
        id: p.host.id, x, y: CL_HEAD + bandH, w: p.w, h: hostH, cols: p.cols,
        items: p.host.items.map((it, i) => ({ id: it.id, x: ox + (i % p.cols) * CELL_W + CELL_W / 2, y: HOST_HEAD + Math.floor(i / p.cols) * CELL_H + 34 })),
      };
      x += p.w + HOST_GAP;
      return box;
    });
    return { id: c.id, x: 0, y: 0, w, h: CL_HEAD + bandH + hostH + CL_PAD + (c.hasControl ? CTRL_LANE : 0), hosts, networks };
  });

  // The rows: the ones a person arranged (`rows`; a source nobody placed yet comes last, in a row of its own), else the clusters flow
  // left to right and wrap when the row is `maxRowW` wide.
  const groups: (typeof boxes)[] = [];
  if (rows) {
    const byId = new Map(boxes.map((b) => [b.id, b]));
    const used = new Set<string>();
    for (const row of rows) {
      const group = row.flatMap((id) => { const b = byId.get(id); return b && !used.has(id) ? (used.add(id), [b]) : []; });
      if (group.length) groups.push(group);
    }
    const rest = boxes.filter((b) => !used.has(b.id));
    if (rest.length) groups.push(rest);
  } else {
    let width = 0;
    for (const b of boxes) {
      if (groups.length && width > 0 && width + b.w > maxRowW) { groups.push([]); width = 0; }
      if (!groups.length) groups.push([]);
      groups[groups.length - 1]!.push(b);
      width += b.w + CL_GAP;
    }
  }
  let y = 0, maxX = 0, rowH = 0;
  for (const group of groups) {
    let x = 0;
    rowH = 0;
    for (const b of group) {
      b.x = x;
      b.y = y;
      x += b.w + CL_GAP;
      rowH = Math.max(rowH, b.h);
      maxX = Math.max(maxX, b.x + b.w);
    }
    y += rowH + CL_GAP;
  }
  if (groups.length) y -= CL_GAP; // the last row has no gap under it

  // everything from here on is in absolute map coordinates
  const items = new Map<string, Placed>();
  const hostMap = new Map<string, HostBox>();
  const clusters: ClusterBox[] = boxes.map((b) => ({
    ...b,
    networks: b.networks.map((it) => {
      const abs: NetworkPlaced = { id: it.id, x: b.x + it.x, y: b.y + it.y, trunk: b.x + it.trunk, bus: b.y + it.bus };
      items.set(abs.id, abs);
      return abs;
    }),
    hosts: b.hosts.map((hb) => {
      const abs: HostBox = { ...hb, x: b.x + hb.x, y: b.y + hb.y, items: hb.items.map((it) => ({ id: it.id, x: b.x + hb.x + it.x, y: b.y + hb.y + it.y })) };
      hostMap.set(abs.id, abs);
      for (const it of abs.items) items.set(it.id, it);
      return abs;
    }),
  }));
  return { clusters, regions: [], items, hosts: hostMap, w: maxX, h: y };
}

/** Is any workload, volume or network of this state drawn? (used to know when the map must be laid out again) */
export const drawnCount = (s: HubState): number => [...s.nodes.values()].filter((n) => (n.kind === 'workload' || n.kind === 'volume' || n.kind === 'network') && isShown(s, n)).length;

export const RG_PAD = 24, RG_HEAD = 44, RG_GAP = 56;

/** Moves everything a layout placed. The maps hold the same objects as `clusters`, so they follow. */
function shift(l: Layout, dx: number, dy: number): void {
  for (const c of l.clusters) {
    c.x += dx; c.y += dy;
    for (const n of c.networks) { n.x += dx; n.y += dy; n.trunk += dx; n.bus += dy; }
    for (const h of c.hosts) { h.x += dx; h.y += dy; for (const it of h.items) { it.x += dx; it.y += dy; } }
  }
}

/**
 * The layout with the regions: each region's clusters are laid out among themselves and framed, the clusters in no region form a
 * block without a frame, and the blocks flow left to right, wrapping at `maxRowW`. Without regions it is `layoutTopology`.
 */
export function layoutGrouped(shape: readonly ShapeCluster[], colsOf: ReadonlyMap<string, number>, maxRowW: number, rows: Rows | undefined, regions: readonly Region[]): Layout {
  const owner = new Map<string, string>();
  for (const r of regions) for (const id of r.clusterIds) if (!owner.has(id)) owner.set(id, r.id);
  const framed = regions.filter((r) => shape.some((c) => owner.get(c.id) === r.id));
  if (!framed.length) return layoutTopology(shape, colsOf, maxRowW, rows);
  const blocks = [...framed.map((r) => ({ region: r, shape: shape.filter((c) => owner.get(c.id) === r.id) })), { region: null, shape: shape.filter((c) => !owner.has(c.id)) }].filter((b) => b.shape.length);
  const out: Layout = { clusters: [], regions: [], items: new Map(), hosts: new Map(), w: 0, h: 0 };
  let x = 0, y = 0, rowH = 0;
  for (const b of blocks) {
    const ids = new Set(b.shape.map((c) => c.id));
    const inner = layoutTopology(b.shape, colsOf, maxRowW, rows?.map((r) => r.filter((id) => ids.has(id))).filter((r) => r.length));
    const pad = b.region ? RG_PAD : 0, head = b.region ? RG_HEAD : 0;
    const w = inner.w + 2 * pad, h = inner.h + head + pad;
    if (x > 0 && x + w > maxRowW) { x = 0; y += rowH + RG_GAP; rowH = 0; }
    shift(inner, x + pad, y + head);
    if (b.region) out.regions.push({ id: b.region.id, name: b.region.name, color: b.region.color, x, y, w, h });
    out.clusters.push(...inner.clusters);
    inner.items.forEach((v, k) => out.items.set(k, v));
    inner.hosts.forEach((v, k) => out.hosts.set(k, v));
    x += w + RG_GAP;
    rowH = Math.max(rowH, h);
    out.w = Math.max(out.w, x - RG_GAP);
    out.h = y + rowH;
  }
  return out;
}

/** The widths clusters may fill before wrapping into the next row, tried by `layoutToFit`. */
export const ROW_WIDTHS = [900, 1200, 1500, 2000, 2600, 3400, 4400, 6000];

/**
 * The layout that fills a screen best. Clusters flow into rows; how wide a row may be decides the shape of the whole map, and a map as
 * tall and narrow as a column of boxes leaves the sides of a landscape screen empty. So each row width is tried and the one that lets the
 * map be shown largest in `view` (the size of the screen, in pixels) wins. Without a size it is the plain layout. Rows a person arranged
 * (`rows`) are used as they are.
 */
export function layoutToFit(shape: readonly ShapeCluster[], colsOf: ReadonlyMap<string, number>, view: { w: number; h: number } | null, rows?: Rows, regions: readonly Region[] = []): Layout {
  const lay = (width: number, r?: Rows) => layoutGrouped(shape, colsOf, width, r, regions);
  if (rows) return lay(MAX_ROW_W, rows); // arranged by a person: the lines are theirs
  if (!view || view.w <= 0 || view.h <= 0) return lay(MAX_ROW_W);
  let best = lay(ROW_WIDTHS[0]), bestK = -1;
  for (const width of ROW_WIDTHS) {
    const l = width === ROW_WIDTHS[0] ? best : lay(width);
    const k = Math.min(view.w / Math.max(l.w, 1), view.h / Math.max(l.h, 1));
    if (k > bestK + 1e-6) { best = l; bestK = k; }
  }
  return best;
}
