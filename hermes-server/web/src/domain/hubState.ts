// Everything the app knows, as ONE immutable value. Events produce a new value (see reducer.ts); nothing is edited in place, so React can
// tell what changed by identity and a test can compare two states.
import type { FlowLine } from '../generated/FlowLine';
import type { HubInfo } from '../generated/HubInfo';
import type { SourceView } from '../generated/SourceView';
import type { Alert, Edge, Node, Uptime } from './model';
import { DEFAULT_SETTINGS, type Settings } from './settings';

export interface UptimeRange {
  id: '1h' | '6h' | '24h' | '7d';
  label: string;
  /** seconds */
  span: number;
  buckets: number;
}

/** Time ranges for the uptime bars. Each cell is span/buckets long. */
export const RANGES: readonly UptimeRange[] = [
  { id: '1h', label: 'Last hour', span: 3600, buckets: 60 },
  { id: '6h', label: 'Last 6 hours', span: 21600, buckets: 72 },
  { id: '24h', label: 'Last 24 hours', span: 86400, buckets: 48 },
  { id: '7d', label: 'Last 7 days', span: 604800, buckets: 84 },
];

export type Link = 'live' | 'lost';

export interface HubState {
  nodes: ReadonlyMap<string, Node>;
  /** parent id -> child ids, in the order the source listed them */
  kids: ReadonlyMap<string, readonly string[]>;
  edges: readonly Edge[];
  alerts: readonly Alert[];
  uptime: ReadonlyMap<string, Uptime>;
  /** short series for the sparklines: `cpu:<node>`, `net:<edge>`, `net:total` */
  history: ReadonlyMap<string, readonly number[]>;
  /** The busiest connections of each source, from its flows agents, and when they came. */
  flows: ReadonlyMap<string, { at: number; lines: readonly FlowLine[] }>;
  settings: Settings;
  sources: readonly SourceView[];
  info: HubInfo;
  /** The hub version this page was loaded from: when the hub is updated under an open page, the two differ. */
  bootVersion: string;
  link: Link;
  range: UptimeRange;
  /** Until the user picks a range, the smallest one that shows all recorded history is used. */
  rangeAuto: boolean;
  /** Set once this session picks its own range (`setRange`): after that, a site-wide default-range change (Settings, live from
   * another session) no longer overrides it — a person looking at 7d should not be yanked back just because someone else changed
   * the default. A session that never picks one (a TV, an admin who has not touched the range picker) keeps following it live. */
  rangePinned: boolean;
}

export const HISTORY_LENGTH = 60;
export const ALERTS_KEPT = 300;

export function initialState(): HubState {
  return {
    nodes: new Map(),
    kids: new Map(),
    edges: [],
    alerts: [],
    uptime: new Map(),
    history: new Map(),
    flows: new Map(),
    settings: DEFAULT_SETTINGS,
    sources: [],
    info: { demo: false, sourceTypes: [], version: '' },
    bootVersion: '',
    link: 'live',
    range: RANGES[2]!,
    rangeAuto: true,
    rangePinned: false,
  };
}
