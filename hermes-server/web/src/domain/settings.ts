// UI settings and alert rules. They live on the hub (SQLite) and are shared by every browser; the rules are also evaluated there.
// This file holds the defaults and the merge of what was saved into them.

export interface Rule {
  id: string;
  name: string;
  target: string;
  sev: 'crit' | 'warn';
  cond: string;
  value: number | null;
  unit: string;
  crit?: number;
  enabled: boolean;
}

export interface Settings {
  /** Rotating between clusters moves the view: opt-in. */
  rotate: boolean;
  rotateSec: number;
  sound: boolean;
  sidebar: boolean;
  /** `false` hides a cluster from the wallboard; a cluster that is not listed is shown. */
  clusters: Record<string, boolean>;
  /** The cluster's own machinery, as opposed to the apps you run: drawn in another colour and hidden unless asked for. */
  showSystem: boolean;
  systemNamespaces: string[];
  rules: Rule[];
  /** Which uptime range a fresh Dashboard/TV session starts on (RANGES' ids); '' keeps today's behaviour: the smallest range that
   * already shows all recorded history, so a young cluster is not shown mostly empty buckets. */
  defaultRange: '' | '1h' | '6h' | '24h' | '7d';
  /** How many days the hub keeps resolved incidents (the ones still open are never forgotten). */
  incidentDays: number;
}

export const DEFAULT_RULES: readonly Rule[] = [
  { id: 'host-down', name: 'Host unreachable', target: 'Host', sev: 'crit', cond: 'No heartbeat for', value: 15, unit: 's', enabled: true },
  { id: 'workload-crash', name: 'Pod / task failing', target: 'Workload', sev: 'crit', cond: 'CrashLoop, image pull error, failed', value: null, unit: '', enabled: true },
  { id: 'volume-usage', name: 'Volume nearly full', target: 'Volume', sev: 'warn', cond: 'Usage ≥', value: 85, unit: '% (critical at 95%)', crit: 95, enabled: true },
  { id: 'iac-drift', name: 'Terraform drift detected', target: 'Host', sev: 'warn', cond: 'State differs from plan', value: null, unit: '', enabled: true },
];

export const DEFAULT_SETTINGS: Settings = {
  rotate: false,
  rotateSec: 20,
  sound: false,
  sidebar: true,
  clusters: {},
  showSystem: false,
  systemNamespaces: ['kube-system', 'kube-public', 'kube-node-lease', 'kube-flannel', 'calico-system', 'calico-apiserver', 'tigera-operator', 'local-path-storage'],
  rules: DEFAULT_RULES.map((r) => ({ ...r })),
  defaultRange: '',
  incidentDays: 30,
};

type Saved = Partial<Omit<Settings, 'rules'>> & { rules?: Partial<Rule>[] };

/** What the hub stored (possibly nothing, possibly from an older version) on top of the defaults. */
export function mergeSettings(saved: unknown): Settings {
  const s = (saved && typeof saved === 'object' ? saved : {}) as Saved;
  const rules = DEFAULT_RULES.map((r) => ({ ...r, ...(s.rules ?? []).find((x) => x.id === r.id) }));
  return { ...DEFAULT_SETTINGS, ...s, clusters: { ...(s.clusters ?? {}) }, rules };
}
