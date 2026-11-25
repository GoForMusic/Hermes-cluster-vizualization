// What the person looking may do, from the rules the hub sent with their login: the same reading of a policy as the hub's (`policy.rs`), used to
// leave out what would be refused. The hub decides; this only keeps the page from offering what will not work.
import type { Grant } from '../generated/Grant';

export type Capability = 'read' | 'list' | 'create' | 'update' | 'delete';

const clean = (path: string): string => path.trim().replace(/^\/+|\/+$/g, '');

/** Does the rule's path cover this path? Exact, a `*` at the end for everything below (or starting so), `+` for one segment. */
export function covers(rule: string, path: string): boolean {
  const have = clean(path) ? clean(path).split('/') : [];
  if (rule === '*') return true;
  const same = (w: string, i: number): boolean => w === '+' || w === have[i];
  if (!rule.endsWith('*')) {
    const want = rule.split('/');
    return want.length === have.length && want.every(same);
  }
  const prefix = rule.slice(0, -1);
  const cut = prefix.lastIndexOf('/');
  const dir = cut < 0 ? '' : prefix.slice(0, cut);
  const partial = cut < 0 ? prefix : prefix.slice(cut + 1);
  const want = dir ? dir.split('/') : [];
  if (have.length < want.length || !want.every(same)) return false;
  const next = have[want.length];
  return next === undefined ? partial === '' : next.startsWith(partial);
}

/** `grants` undefined: a hub that does not say (an older one): nothing is hidden. Deny beats everything; `list` also lets a read through. */
export function can(grants: readonly Grant[] | undefined, capability: Capability, path: string): boolean {
  if (!grants) return true;
  const rules = grants.filter((g) => covers(g.path, path));
  if (rules.some((g) => g.capabilities.includes('deny'))) return false;
  return rules.some((g) => g.capabilities.includes(capability) || (capability === 'read' && g.capabilities.includes('list')));
}

/**
 * May the person see some cluster (the map, its alerts)? A rule for `clusters/<id>` (or `clusters/*`, or `*`) that gives `read` and is not a `deny`.
 * `grants` undefined: a hub that does not say: yes.
 */
export function canSeeClusters(grants: readonly Grant[] | undefined): boolean {
  if (!grants) return true;
  return grants.some((g) => !g.capabilities.includes('deny') && (g.capabilities.includes('read') || g.capabilities.includes('list')) && (g.path === '*' || g.path === 'clusters' || g.path.startsWith('clusters/')))
    || can(grants, 'read', 'snapshot');
}
