// Which clusters sit on the same line of the map: automatic until a person drags one somewhere else. Pure, so it can be tested without a screen.
import type { Rows } from './layout';

/** A cluster to place. Only its position matters here. */
interface Box { id: string; x: number; y: number; w: number; h: number }

/** The lines the clusters are on now, from where they are drawn: the ones with the same top are one line, left to right. */
export function rowsOf(boxes: readonly Box[]): string[][] {
  const lines = new Map<number, Box[]>();
  for (const b of boxes) lines.set(b.y, [...(lines.get(b.y) ?? []), b]);
  return [...lines.entries()].sort(([a], [b]) => a - b).map(([, line]) => line.sort((a, b) => a.x - b.x).map((b) => b.id));
}

export type Side = 'l' | 'r' | 't' | 'b';

/**
 * `rows` with `id` dropped on `target`: on its left or right it joins the line of `target`, just before or just after it; above or below
 * it it gets a line of its own there. A line left empty by the move is gone. Ids it does not know leave `rows` as they are.
 */
export function arrange(rows: Rows, id: string, target: string, side: Side): string[][] {
  const flat = rows.flat();
  if (id === target || !flat.includes(id) || !flat.includes(target)) return rows.map((r) => [...r]);
  const without = rows.map((r) => r.filter((x) => x !== id));
  const line = without.findIndex((r) => r.includes(target));
  const at = without[line]!.indexOf(target);
  const next = without.map((r) => [...r]);
  if (side === 'l' || side === 'r') next[line]!.splice(at + (side === 'r' ? 1 : 0), 0, id);
  else next.splice(line + (side === 'b' ? 1 : 0), 0, [id]);
  return next.filter((r) => r.length);
}

/** Which side of a box a drop is on, from where inside it the pointer is: the axis it is furthest along decides. */
export function dropSide(box: Omit<Box, 'id'>, px: number, py: number): Side {
  const dx = (px - (box.x + box.w / 2)) / box.w, dy = (py - (box.y + box.h / 2)) / box.h;
  return Math.abs(dx) >= Math.abs(dy) ? (dx > 0 ? 'r' : 'l') : dy > 0 ? 'b' : 't';
}
