// The order of the clusters on the map: automatic (the order of the sources) until a person drags one somewhere else.
// Pure, so it can be tested without a screen.

/** `items` in the order `order` names; the ones it does not name (a new source) follow, in their own order. */
export function orderClusters<T extends { id: string }>(items: readonly T[], order: readonly string[]): T[] {
  const rank = new Map(order.map((id, i) => [id, i]));
  const named = items.filter((c) => rank.has(c.id)).sort((a, b) => rank.get(a.id)! - rank.get(b.id)!);
  return [...named, ...items.filter((c) => !rank.has(c.id))];
}

/** The ids with `id` moved next to `target`: just before it, or just after it. Unknown ids leave the order as it is. */
export function moveTo(ids: readonly string[], id: string, target: string, after = false): string[] {
  if (id === target || !ids.includes(id) || !ids.includes(target)) return [...ids];
  const next = ids.filter((x) => x !== id);
  next.splice(next.indexOf(target) + (after ? 1 : 0), 0, id);
  return next;
}

/** Which side of a box a drop is on, from where inside it the pointer is: the axis it is furthest along decides. */
export function dropSide(box: { x: number; y: number; w: number; h: number }, px: number, py: number): 'l' | 'r' | 't' | 'b' {
  const dx = (px - (box.x + box.w / 2)) / box.w, dy = (py - (box.y + box.h / 2)) / box.h;
  return Math.abs(dx) >= Math.abs(dy) ? (dx > 0 ? 'r' : 'l') : dy > 0 ? 'b' : 't';
}
