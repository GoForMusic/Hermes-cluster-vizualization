import { useCallback, useEffect, useRef, useState } from 'react';

interface Options {
  enabled: boolean;
  seconds: number;
  /** While a critical incident is open the view stays where it is. */
  holding: boolean;
  /** The views to cycle through after "all": cluster ids. */
  clusterIds: readonly string[];
}

/** Which cluster the wallboard is looking at (`null`: all of them), and the timer that moves it on. */
export function useRotation({ enabled, seconds, holding, clusterIds }: Options) {
  const [focusId, setFocusId] = useState<string | null>(null);
  const pausedUntil = useRef(0);
  const index = useRef(0);
  const latest = useRef({ enabled, holding, clusterIds });
  latest.current = { enabled, holding, clusterIds };

  // a cluster that disappeared (removed, hidden) cannot stay in focus
  useEffect(() => { if (focusId && !clusterIds.includes(focusId)) setFocusId(null); }, [clusterIds, focusId]);

  useEffect(() => {
    const t = setInterval(() => {
      const { enabled: on, holding: hold, clusterIds: ids } = latest.current;
      if (!on || hold || Date.now() < pausedUntil.current) return;
      const order: (string | null)[] = [null, ...ids];
      index.current = (index.current + 1) % order.length;
      setFocusId(order[index.current] ?? null);
    }, Math.max(5, seconds) * 1000);
    return () => clearInterval(t);
  }, [seconds]);

  /** A person picked a view: leave it alone for a minute. */
  const choose = useCallback((id: string | null) => { pausedUntil.current = Date.now() + 60_000; setFocusId(id); }, []);
  return { focusId, choose };
}
