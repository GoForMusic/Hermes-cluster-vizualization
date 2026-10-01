// The dashboard detail's bucket-click / time-window filtering: pick a cell on the heartbeat bar and the incident list narrows to
// what happened right then. Pulled out of `Detail` so that component stays a rendering component, not a state machine.
import { useEffect, useState } from 'react';
import type { UptimeRange } from '../../../domain/hubState';
import type { Alert } from '../../../domain/model';
import { incidentsFor } from '../../../domain/selectors';
import { useWholeState } from '../../../state/context';

export interface IncidentWindow {
  /** The bucket index picked on the heartbeat bar, if any. */
  picked: number | null;
  /** Toggles a bucket: picking the one already picked clears it. */
  pick: (i: number) => void;
  /** Unconditionally clears the pick (the "Show all" button). */
  clear: () => void;
  pickedWindow: [number, number] | null;
  /** The incidents to list: everything in view, or just what falls inside the picked bucket. */
  shown: Alert[];
  /** The node's own status at the picked bucket, when nothing was open then — explains an empty list. */
  pickedStatus: string | null;
}

export function useIncidentWindow(nodeId: string, range: UptimeRange, bars: readonly string[] | undefined): IncidentWindow {
  const state = useWholeState();
  const [picked, setPicked] = useState<number | null>(null);
  useEffect(() => setPicked(null), [nodeId, range.id]); // a different node or a different zoom: the bucket indices mean something else now

  // Scoped to the same window as the heartbeat bar: a workload's own alert, or the reason its host/cluster was down at the time
  // (a pod's red cell usually comes from its host, not from the pod itself — without this its incident history looked empty).
  const windowStart = Date.now() - range.span * 1000;
  const alerts = incidentsFor(state, nodeId, windowStart, Date.now());
  const bucketMs = (range.span * 1000) / range.buckets;
  const bucketWindow = (i: number): [number, number] => {
    const end = Date.now() - (range.buckets - 1 - i) * bucketMs;
    return [end - bucketMs, end];
  };
  const pickedWindow = picked != null ? bucketWindow(picked) : null;
  // Clicking a cell narrows the list to what happened right then — helpful once there is more than a handful of incidents in view.
  const shown = pickedWindow ? alerts.filter((a) => a.ts <= pickedWindow[1] && (a.resolvedTs == null || a.resolvedTs >= pickedWindow[0])) : alerts.slice(0, 8);
  const pickedStatus = picked != null ? (bars?.[picked] ?? 'nodata') : null;

  return {
    picked,
    pick: (i) => setPicked((cur) => (cur === i ? null : i)),
    clear: () => setPicked(null),
    pickedWindow,
    shown,
    pickedStatus,
  };
}
