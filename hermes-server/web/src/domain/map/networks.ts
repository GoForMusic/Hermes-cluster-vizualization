// The colour that tells the networks apart on the map. Pure.
import type { HubState } from '../hubState';

/** Colours for networks, none of them one of the state colours (cyan, yellow, red, green, grey). */
export const NETWORK_COLORS = ['#b18cff', '#ff9f5a', '#ff7ac8', '#5ab8ff', '#d7e05a', '#7be0c3'] as const;

export interface NetworkTags {
  /** the colour of each network (by its id) */
  colors: ReadonlyMap<string, string>;
}

export function networkTags(s: HubState): NetworkTags {
  const colors = new Map<string, string>();
  for (const n of s.nodes.values()) if (n.kind === 'network') colors.set(n.id, NETWORK_COLORS[colors.size % NETWORK_COLORS.length]!);
  return { colors };
}
