// The traffic and control links between things on the map: routed with right angles around everything drawn.
import type { Edge } from '../model';
import { CELL_W, CL_HEAD, CL_PAD, HOST_HEAD, type Layout, type NetworkPlaced } from './layout';
import { Router, type Point } from './router';

export interface Geometry {
  /** SVG path */
  d: string;
  /** where the label goes */
  mid: { x: number; y: number };
  /** where the arrowhead is and which way it points */
  end: { x: number; y: number; dx: number; dy: number };
}
export interface LinkGeometry extends Geometry {
  edgeId: string;
}

const GRID = 160, GRID_PAD = 80;

/** Everything a traffic link must not run through: unit plates and their labels, host and cluster headers, the control-plane lane. */
function blockObstacles(router: Router, layout: Layout): void {
  for (const b of layout.clusters) {
    router.block(b.x, b.y, b.x + b.w, b.y + CL_HEAD - 2);
    const first = b.hosts[0];
    const bottom = first ? first.y + first.h : b.y + b.h;
    if (b.y + b.h - bottom > CL_PAD + 1) router.block(b.x, bottom + 2, b.x + b.w, b.y + b.h);
    if (b.networks.length && first) router.block(b.x, b.y + CL_HEAD - 2, b.x + b.w, first.y - 2); // the networks and their bus lines
    for (const hb of b.hosts) {
      router.block(hb.x, hb.y, hb.x + hb.w, hb.y + HOST_HEAD - 2);
      for (const it of hb.items) {
        router.block(it.x - 30, it.y - 22, it.x + 30, it.y + 22); // the plate
        router.block(it.x - 45, it.y + 23, it.x + 45, it.y + 67); // name, subtitle and network
      }
    }
  }
}

const dedupe = (pts: Point[]): Point[] =>
  pts.filter((p, i) => {
    if (i === 0 || i === pts.length - 1) return true;
    const [x0, y0] = pts[i - 1]!, [x1, y1] = p, [x2, y2] = pts[i + 1]!;
    return (x1 - x0) * (y2 - y1) !== (y1 - y0) * (x2 - x1);
  });

const d = (pts: readonly Point[]): string => 'M' + pts.map((p) => `${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join('L');

/** Routes one link with right angles only. Tries every combination of port sides and keeps the cheapest. */
function routeEdge(router: Router, a: { x: number; y: number }, b: { x: number; y: number }): Geometry {
  const dx = b.x - a.x;
  const combos: [1 | -1, 1 | -1][] =
    dx > 40 ? [[1, -1], [1, 1], [-1, -1], [-1, 1]] : dx < -40 ? [[-1, 1], [-1, -1], [1, 1], [1, -1]] : [[1, 1], [-1, -1], [1, -1], [-1, 1]];
  let best: { pts: Point[]; cost: number; sa: 1 | -1; sb: 1 | -1 } | null = null;
  for (const [sa, sb] of combos) {
    const r = router.find({ x: a.x, y: a.y, side: sa }, { x: b.x, y: b.y, side: sb });
    if (r && (!best || r.cost < best.cost)) best = { ...r, sa, sb };
  }
  let pts: Point[];
  if (best) {
    const inner = best.pts.map((p): [number, number] => [p[0], p[1]]);
    // snap the first and last horizontal runs onto the plate's centre line so that the joins are exactly square
    const oy0 = inner[0]![1];
    inner[0]![1] = a.y;
    if (inner.length > 1 && inner[1]![1] === oy0) inner[1]![1] = a.y;
    const n = inner.length - 1, oyn = inner[n]![1];
    inner[n]![1] = b.y;
    if (n > 0 && inner[n - 1]![1] === oyn) inner[n - 1]![1] = b.y;
    pts = [[a.x + best.sa * 27, a.y], ...inner, [b.x + best.sb * 27, b.y]];
    router.commit(pts);
  } else {
    const mx = (a.x + b.x) / 2; // no free corridor: a plain square fallback
    pts = [[a.x + (dx >= 0 ? 27 : -27), a.y], [mx, a.y], [mx, b.y], [b.x - (dx >= 0 ? 27 : -27), b.y]];
  }
  pts = dedupe(pts);

  // the label sits in the middle of the longest run
  let bestLen = -1;
  let mid = { x: pts[0]![0], y: pts[0]![1] };
  for (let i = 1; i < pts.length; i++) {
    const [x0, y0] = pts[i - 1]!, [x1, y1] = pts[i]!;
    const len = Math.abs(x1 - x0) + Math.abs(y1 - y0);
    if (len > bestLen) { bestLen = len; mid = { x: (x0 + x1) / 2, y: (y0 + y1) / 2 }; }
  }
  const [px, py] = pts[pts.length - 2]!, [qx, qy] = pts[pts.length - 1]!;
  const len = Math.hypot(qx - px, qy - py) || 1;
  return { d: d(pts), mid, end: { x: qx, y: qy, dx: (qx - px) / len, dy: (qy - py) / len } };
}

/** A filled triangle whose tip is at `end`, pointing along its direction. */
export function arrowPath({ x, y, dx, dy }: Geometry['end'], size = 10, half = 4.6): string {
  const bx = x - dx * size, by = y - dy * size, nx = -dy * half, ny = dx * half;
  return `M${x.toFixed(1)} ${y.toFixed(1)}L${(bx + nx).toFixed(1)} ${(by + ny).toFixed(1)}L${(bx - nx).toFixed(1)} ${(by - ny).toFixed(1)}Z`;
}

/**
 * A network's line to one of its members: out of the plate's right corner, down the free corridor beside it (between two columns of plates
 * nothing is drawn) to the network's bus line, along the bus, down the corridor to the left of the member and into its side. `k` of `count`
 * shifts the last corridor so that the lines of several networks run side by side and not on top of each other.
 */
function branch(net: NetworkPlaced, to: { x: number; y: number }, k: number, count: number): Geometry {
  const cx = to.x - CELL_W / 2 + (k - (count - 1) / 2) * 4;
  const ex = to.x - 27;
  return { d: `M${net.x + 20} ${net.y} H${net.trunk} V${net.bus} H${cx} V${to.y} H${ex}`, mid: { x: cx, y: (net.bus + to.y) / 2 }, end: { x: ex, y: to.y, dx: 1, dy: 0 } };
}

/**
 * A line from one network to another (outside to an Ingress, an Ingress to a Service). Next to each other in the same row: straight from side
 * to side. Further along the same row: over the top of the plates in between (a higher run the more it skips, so they do not overlap). Anywhere
 * else: down to the first one's bus, along it, and up the corridor just left of the second plate.
 */
function between(a: NetworkPlaced, b: NetworkPlaced): Geometry {
  const ex = b.x - 20, sx = a.x + 20;
  const skipped = Math.round((b.x - a.x) / CELL_W) - 1;
  if (a.y === b.y && skipped === 0) return { d: `M${sx} ${a.y} H${ex}`, mid: { x: (sx + ex) / 2, y: a.y }, end: { x: ex, y: b.y, dx: 1, dy: 0 } };
  if (a.y === b.y && skipped > 0) {
    const top = a.y - 24 - (skipped - 1) * 5;
    const x1 = sx + 10, x2 = ex - 10;
    return { d: `M${sx} ${a.y} H${x1} V${top} H${x2} V${b.y} H${ex}`, mid: { x: (x1 + x2) / 2, y: top }, end: { x: ex, y: b.y, dx: 1, dy: 0 } };
  }
  const cx = b.x - CELL_W / 2 + 8;
  return { d: `M${sx} ${a.y} H${a.trunk} V${a.bus} H${cx} V${b.y} H${ex}`, mid: { x: cx, y: (a.bus + b.y) / 2 }, end: { x: ex, y: b.y, dx: 1, dy: 0 } };
}

/** The geometry of every link whose two ends are on the map. Control links are U-shapes under the hosts, traffic links are routed. */
export function computeLinks(layout: Layout, edges: readonly Edge[]): LinkGeometry[] {
  const router = new Router(layout.w, layout.h);
  blockObstacles(router, layout);
  const lanes = new Map<string, number>();
  const nets = new Map<string, { at: NetworkPlaced; k: number; count: number }>();
  for (const c of layout.clusters) c.networks.forEach((n, k) => nets.set(n.id, { at: n, k, count: c.networks.length }));
  const out: LinkGeometry[] = [];
  // routes go last: they must not take the corridors that traffic links need
  for (const e of [...edges.filter((e) => e.type !== 'route'), ...edges.filter((e) => e.type === 'route')]) {
    const net = nets.get(e.from);
    if (net) { // a network to something on it, or behind it
      const other = nets.get(e.to);
      if (other) {
        out.push({ edgeId: e.id, ...between(net.at, other.at) });
        continue;
      }
      const b = layout.items.get(e.to);
      if (b) out.push({ edgeId: e.id, ...branch(net.at, b, net.k, net.count) });
      continue;
    }
    if (e.type === 'control') {
      const a = layout.hosts.get(e.from), b = layout.hosts.get(e.to);
      if (!a || !b) continue;
      const idx = lanes.get(e.from) ?? 0;
      lanes.set(e.from, idx + 1);
      const yb = a.y + a.h, ly = yb + 10 + idx * 16;
      const x1 = a.x + a.w / 2 + idx * 18, x2 = b.x + b.w / 2;
      out.push({ edgeId: e.id, d: `M${x1} ${yb} V${ly} H${x2} V${yb}`, mid: { x: (x1 + x2) / 2, y: ly + 11 }, end: { x: x2, y: yb, dx: 0, dy: -1 } });
    } else {
      const a = layout.items.get(e.from), b = layout.items.get(e.to);
      if (!a || !b) continue;
      out.push({ edgeId: e.id, ...routeEdge(router, a, b) });
    }
  }
  return out;
}

/** A reference on the map's coordinate grid, like "C07". */
export function gridRef(x: number, y: number): string {
  const col = Math.max(0, Math.floor((x + GRID_PAD) / GRID));
  const row = Math.max(0, Math.floor((y + GRID_PAD) / GRID));
  return `${String.fromCharCode(65 + (col % 26))}${String(row + 1).padStart(2, '0')}`;
}

export const GRID_SIZE = GRID;
export const GRID_MARGIN = GRID_PAD;
