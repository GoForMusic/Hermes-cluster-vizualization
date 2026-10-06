// The order a person gave the clusters by dragging them. A convenience of this browser: kept here, never sent to the hub.
import { useCallback, useState } from 'react';
import { moveTo } from '../../domain/map/order';

const KEY = 'hermes.clusterOrder';

function load(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(raw) ? raw.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return []; // private window or blocked storage: it just does not remember
  }
}

export interface ClusterOrder {
  order: readonly string[];
  /** Puts `id` just before or just after `target`, given the clusters as they are drawn now. */
  place: (id: string, target: string, shown: readonly string[], after: boolean) => void;
  /** Back to the automatic order. */
  clear: () => void;
}

export function useClusterOrder(): ClusterOrder {
  const [order, setOrder] = useState<readonly string[]>(load);
  const save = (next: readonly string[]) => {
    try { localStorage.setItem(KEY, JSON.stringify(next)); } catch { /* not remembered */ }
    setOrder(next);
  };
  const place = useCallback((id: string, target: string, shown: readonly string[], after: boolean) => save(moveTo(shown, id, target, after)), []);
  const clear = useCallback(() => save([]), []);
  return { order, place, clear };
}
