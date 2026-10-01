// How a state changes: the boot data and every event of the live stream. Pure: (state, event, now) -> state.
import type { HubEvent } from '../generated/HubEvent';
import type { HubInfo } from '../generated/HubInfo';
import type { Node as WireNode } from '../generated/Node';
import type { Edge as WireEdge } from '../generated/Edge';
import type { SourceView } from '../generated/SourceView';
import { deriveStatuses } from './derive';
import { ALERTS_KEPT, HISTORY_LENGTH, RANGES, type HubState } from './hubState';
import { parseEdge, parseNode, type Alert, type Node, type NodeMeta } from './model';
import { mergeSettings, type Settings } from './settings';
import { isOwn } from './status';

export interface Boot {
  nodes: WireNode[];
  edges: WireEdge[];
  info: HubInfo;
  settings: unknown;
  alerts: Alert[];
  sources: SourceView[];
}

/** What came out of one event: the new state, and what the outside world should hear about it. */
export interface Reduction {
  state: HubState;
  /** An alert that just opened (not one that changed): the wallboard announces it. */
  newAlert?: Alert;
  /** The list of sources changed on the hub: it has to be fetched again. */
  refreshSources?: boolean;
}

function topology(state: HubState, wireNodes: WireNode[], wireEdges: WireEdge[], now: number): HubState {
  const parsed = wireNodes.map(parseNode);
  const kids = new Map<string, string[]>();
  for (const n of parsed) {
    if (!n.parent) continue;
    const list = kids.get(n.parent) ?? [];
    list.push(n.id);
    kids.set(n.parent, list);
  }
  const nodes = deriveStatuses(new Map(parsed.map((n) => [n.id, n])), kids, state.settings, now);
  return { ...state, nodes, kids, edges: wireEdges.map(parseEdge) };
}

/** Where a session's range follows the shared default (Admin → Settings): applied at boot and on every live settings change, unless
 * this session has pinned its own range (`setRange` — a person looking at 7d should not be yanked back just because someone else
 * changed the default). '' keeps the smart auto-pick that grows the range as history piles up. */
function withDefaultRange(state: HubState, settings: Settings): Pick<HubState, 'range' | 'rangeAuto'> {
  if (state.rangePinned) return { range: state.range, rangeAuto: state.rangeAuto };
  const range = RANGES.find((r) => r.id === settings.defaultRange) ?? state.range;
  const rangeAuto = settings.defaultRange ? false : state.rangeAuto;
  return { range, rangeAuto };
}

export function boot(state: HubState, data: Boot, now: number): HubState {
  const settings = mergeSettings(data.settings);
  const { range, rangeAuto } = withDefaultRange(state, settings);
  return topology({ ...state, settings, range, rangeAuto, alerts: data.alerts, info: data.info, bootVersion: data.info.version, sources: data.sources }, data.nodes, data.edges, now);
}

const derived = (state: HubState, nodes: Map<string, Node>, now: number): HubState => ({ ...state, nodes: deriveStatuses(nodes, state.kids, state.settings, now) });

function pushSeries(history: Map<string, readonly number[]>, key: string, value: number): void {
  const list = [...(history.get(key) ?? []), value];
  history.set(key, list.length > HISTORY_LENGTH ? list.slice(list.length - HISTORY_LENGTH) : list);
}

export function upsertAlert(alerts: readonly Alert[], alert: Alert): Alert[] {
  const list = alerts.some((a) => a.id === alert.id) ? alerts.map((a) => (a.id === alert.id ? alert : a)) : [alert, ...alerts];
  return list.sort((x, y) => y.ts - x.ts).slice(0, ALERTS_KEPT);
}

export function reduce(state: HubState, event: HubEvent, now: number): Reduction {
  switch (event.type) {
    case 'snapshot':
      return { state: topology(state, event.nodes, event.edges, now) };

    case 'status': {
      const n = state.nodes.get(event.id);
      if (!n || !isOwn(event.own)) return { state };
      return { state: derived(state, new Map(state.nodes).set(n.id, { ...n, own: event.own, reason: event.reason ?? '' }), now) };
    }

    case 'meta': {
      const n = state.nodes.get(event.id);
      if (!n) return { state };
      return { state: { ...state, nodes: new Map(state.nodes).set(n.id, { ...n, meta: { ...n.meta, ...(event.meta as NodeMeta) } }) } };
    }

    case 'flows':
      return { state: { ...state, flows: new Map(state.flows).set(event.source, { at: now, lines: event.flows }) } };

    case 'metrics': {
      const nodes = new Map(state.nodes);
      const history = new Map(state.history);
      for (const [id, m] of Object.entries(event.nodes)) {
        const n = nodes.get(id);
        if (!n) continue;
        nodes.set(id, { ...n, m: { ...n.m, ...m } });
        const load = m.cpuMilli ?? m.cpu; // pods chart absolute usage, hosts a percentage
        if (load != null) pushSeries(history, `cpu:${id}`, load);
      }
      const known = new Map(state.edges.map((e) => [e.id, e]));
      const moved = new Map<string, number>();
      let total = 0;
      for (const [id, mbps] of Object.entries(event.edges)) {
        if (!known.has(id)) continue;
        moved.set(id, mbps);
        total += mbps;
        pushSeries(history, `net:${id}`, mbps);
      }
      pushSeries(history, 'net:total', total);
      const edges = moved.size ? state.edges.map((e) => (moved.has(e.id) ? { ...e, mbps: moved.get(e.id)! } : e)) : state.edges;
      return { state: { ...derived(state, nodes, now), edges, history } };
    }

    case 'alert':
      return { state: { ...state, alerts: upsertAlert(state.alerts, event.alert) }, newAlert: event.isNew ? event.alert : undefined };

    case 'settings': {
      const settings = mergeSettings(event.settings);
      const { range, rangeAuto } = withDefaultRange(state, settings);
      return { state: derived({ ...state, settings, range, rangeAuto }, new Map(state.nodes), now) };
    }

    case 'sources':
      return { state, refreshSources: true };
  }
}
