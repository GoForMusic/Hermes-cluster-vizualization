// The columns a person gave to a host's box by dragging its corner. A convenience of this browser: kept here, never sent to the hub.
import { useCallback, useState } from 'react';

const KEY = 'hermes.hostCols';

function load(): Map<string, number> {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    if (!Array.isArray(raw)) return new Map();
    return new Map(raw.filter((e): e is [string, number] => Array.isArray(e) && typeof e[0] === 'string' && Number.isFinite(e[1])));
  } catch {
    return new Map(); // private window or blocked storage: it just does not remember
  }
}

export interface HostCols {
  cols: ReadonlyMap<string, number>;
  /** `null` goes back to the automatic layout. */
  set: (hostId: string, cols: number | null) => void;
}

export function useHostCols(): HostCols {
  const [cols, setCols] = useState<ReadonlyMap<string, number>>(load);
  const set = useCallback((hostId: string, n: number | null) => {
    setCols((prev) => {
      const next = new Map(prev);
      if (n === null) next.delete(hostId); else next.set(hostId, n);
      try { localStorage.setItem(KEY, JSON.stringify([...next])); } catch { /* not remembered */ }
      return next;
    });
  }, []);
  return { cols, set };
}
