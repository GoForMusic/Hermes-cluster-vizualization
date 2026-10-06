// The lines a person gave the clusters by dragging them. A convenience of this browser: kept here, never sent to the hub.
import { useCallback, useState } from 'react';
import type { Rows } from '../../domain/map/layout';

const KEY = 'hermes.clusterRows';

function load(): Rows | null {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (!Array.isArray(raw) || !raw.every((r) => Array.isArray(r) && r.every((id) => typeof id === 'string'))) return null;
    return raw as string[][];
  } catch {
    return null; // private window or blocked storage: it just does not remember
  }
}

export interface ClusterRows {
  /** `null` is the automatic arrangement. */
  rows: Rows | null;
  set: (rows: Rows) => void;
  /** Back to the automatic arrangement. */
  clear: () => void;
}

export function useClusterRows(): ClusterRows {
  const [rows, setRows] = useState<Rows | null>(load);
  const save = (next: Rows | null) => {
    try { if (next) localStorage.setItem(KEY, JSON.stringify(next)); else localStorage.removeItem(KEY); } catch { /* not remembered */ }
    setRows(next);
  };
  const set = useCallback((next: Rows) => save(next), []);
  const clear = useCallback(() => save(null), []);
  return { rows, set, clear };
}
