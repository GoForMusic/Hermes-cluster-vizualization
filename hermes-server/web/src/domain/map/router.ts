// Orthogonal (right-angle) link router: A* on a coarse grid with obstacles, a bend penalty and a congestion penalty, so parallel links
// spread over different corridors instead of stacking.

const CELL = 6;
const TURN = 10; // cost of a bend
const USE = 4; // cost per link already using a cell
const DIRS: readonly (readonly [number, number])[] = [[1, 0], [0, 1], [-1, 0], [0, -1]]; // right, down, left, up

export type Point = readonly [number, number];
export interface Port {
  x: number;
  y: number;
  /** +1 leaves/enters through the right edge, -1 through the left edge */
  side: 1 | -1;
}
export interface Route {
  /** the corners of the path, both ends included */
  pts: Point[];
  cost: number;
}

/** A binary min-heap of [priority, state]. */
class Heap {
  private items: [number, number][] = [];
  get size(): number { return this.items.length; }

  push(f: number, s: number): void {
    const a = this.items;
    let i = a.push([f, s]) - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      if (a[p]![0] <= a[i]![0]) break;
      [a[p], a[i]] = [a[i]!, a[p]!];
      i = p;
    }
  }

  pop(): [number, number] {
    const a = this.items;
    const top = a[0]!;
    const last = a.pop()!;
    if (a.length) {
      a[0] = last;
      let i = 0;
      for (;;) {
        const l = 2 * i + 1, r = l + 1;
        let m = i;
        if (l < a.length && a[l]![0] < a[m]![0]) m = l;
        if (r < a.length && a[r]![0] < a[m]![0]) m = r;
        if (m === i) break;
        [a[m], a[i]] = [a[i]!, a[m]!];
        i = m;
      }
    }
    return top;
  }
}

export class Router {
  private readonly ox = -60;
  private readonly oy = -60;
  private readonly cols: number;
  private readonly rows: number;
  private readonly blocked: Uint8Array;
  private readonly usage: Uint8Array;

  constructor(w: number, h: number) {
    this.cols = Math.ceil((w + 120) / CELL);
    this.rows = Math.ceil((h + 120) / CELL);
    this.blocked = new Uint8Array(this.cols * this.rows);
    this.usage = new Uint8Array(this.cols * this.rows);
  }

  private cell(x: number, y: number): [number, number] {
    return [Math.floor((x - this.ox) / CELL), Math.floor((y - this.oy) / CELL)];
  }

  private point(c: number, r: number): Point {
    return [this.ox + (c + 0.5) * CELL, this.oy + (r + 0.5) * CELL];
  }

  /** Nothing may run through this rectangle. */
  block(x0: number, y0: number, x1: number, y1: number): void {
    const [c0, r0] = this.cell(x0, y0);
    const [c1, r1] = this.cell(x1, y1);
    for (let r = Math.max(0, r0); r <= Math.min(this.rows - 1, r1); r++) {
      for (let c = Math.max(0, c0); c <= Math.min(this.cols - 1, c1); c++) this.blocked[r * this.cols + c] = 1;
    }
  }

  /** The cheapest path from one port to another, or null when there is no free corridor. */
  find(a: Port, b: Port): Route | null {
    const { cols, rows, blocked, usage } = this;
    const [sc, sr] = this.cell(a.x + a.side * 33, a.y);
    const [tc, tr] = this.cell(b.x + b.side * 33, b.y);
    if (blocked[sr * cols + sc] || blocked[tr * cols + tc]) return null;
    const heur = (c: number, r: number): number => Math.abs(c - tc) + Math.abs(r - tr);

    const g = new Float32Array(cols * rows * 4).fill(Infinity);
    const prev = new Int32Array(cols * rows * 4).fill(-1);
    const heap = new Heap();
    const start = (sr * cols + sc) * 4 + (a.side === 1 ? 0 : 2);
    g[start] = 0;
    heap.push(heur(sc, sr), start);

    while (heap.size) {
      const [f, s] = heap.pop();
      const cell = s >> 2, d = s & 3;
      const c = cell % cols, r = (cell - c) / cols;
      if (f > g[s]! + heur(c, r) + 1e-3) continue; // a stale entry
      if (c === tc && r === tr) return { pts: this.corners(prev, s), cost: g[s]! };
      for (let nd = 0; nd < 4; nd++) {
        if (nd === ((d + 2) & 3)) continue; // no U-turns
        const [dc, dr] = DIRS[nd]!;
        const nc = c + dc, nr = r + dr;
        if (nc < 0 || nr < 0 || nc >= cols || nr >= rows) continue;
        const ni = nr * cols + nc;
        if (blocked[ni]) continue;
        const ns = ni * 4 + nd;
        const ng = g[s]! + 1 + (nd !== d ? TURN : 0) + usage[ni]! * USE;
        if (ng < g[ns]!) {
          g[ns] = ng;
          prev[ns] = s;
          heap.push(ng + heur(nc, nr), ns);
        }
      }
    }
    return null;
  }

  private corners(prev: Int32Array, end: number): Point[] {
    const cells: number[] = [];
    for (let s = end; s !== -1; s = prev[s]!) cells.push(s);
    cells.reverse();
    const full = cells.map((s) => {
      const cell = s >> 2, c = cell % this.cols;
      return this.point(c, (cell - c) / this.cols);
    });
    // keep only the real corners (where the direction changes), plus both ends
    const out: Point[] = [full[0]!];
    for (let i = 1; i < full.length - 1; i++) {
      const [x0, y0] = full[i - 1]!, [x1, y1] = full[i]!, [x2, y2] = full[i + 1]!;
      if ((x1 - x0) * (y2 - y1) !== (y1 - y0) * (x2 - x1)) out.push(full[i]!);
    }
    out.push(full[full.length - 1]!);
    return out;
  }

  /** Marks a routed polyline as used (and its neighbours a little) so that later links avoid it. */
  commit(pts: readonly Point[]): void {
    for (let i = 1; i < pts.length; i++) {
      const [x0, y0] = pts[i - 1]!, [x1, y1] = pts[i]!;
      const n = Math.max(1, Math.ceil(Math.hypot(x1 - x0, y1 - y0) / CELL));
      for (let k = 0; k <= n; k++) {
        const [c, r] = this.cell(x0 + ((x1 - x0) * k) / n, y0 + ((y1 - y0) * k) / n);
        for (let dr = -1; dr <= 1; dr++) {
          for (let dc = -1; dc <= 1; dc++) {
            const cc = c + dc, rr = r + dr;
            if (cc < 0 || rr < 0 || cc >= this.cols || rr >= this.rows) continue;
            const idx = rr * this.cols + cc;
            this.usage[idx] = Math.min(9, this.usage[idx]! + (dr === 0 && dc === 0 ? 2 : 1));
          }
        }
      }
    }
  }
}
