// Small worlds for the tests: a node as the hub sends it, and a state built from them.
import type { Edge as WireEdge } from '../generated/Edge';
import type { Node as WireNode } from '../generated/Node';
import { initialState, type HubState } from '../domain/hubState';
import { boot } from '../domain/reducer';

export const NOW = 1_700_000_000_000;

export function wireNode(id: string, kind: string, parent: string | null, extra: Partial<WireNode> = {}): WireNode {
  return { id, kind, name: id, parent, provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta: {}, ...extra };
}

export function wireEdge(id: string, from: string, to: string, extra: Partial<WireEdge> = {}): WireEdge {
  return { id, from, to, base: 1, mbps: 1, type: 'traffic', ...extra };
}

/** cluster c > hosts h1, h2 > workloads w1, w2 (on h1) and w3 (on h2) */
export function world(overrides: Record<string, Partial<WireNode>> = {}, edges: WireEdge[] = []): HubState {
  const nodes = [
    wireNode('c', 'cluster', null, { meta: { version: 'v1' } }),
    wireNode('h1', 'host', 'c', { meta: { role: 'control-plane', ip: '10.0.0.1' } }),
    wireNode('h2', 'host', 'c', { meta: { role: 'worker', ip: '10.0.0.2' } }),
    wireNode('w1', 'workload', 'h1', { meta: { type: 'Deployment', ns: 'default' } }),
    wireNode('w2', 'workload', 'h1', { meta: { type: 'Deployment', ns: 'kube-system' } }),
    wireNode('w3', 'workload', 'h2', { meta: { type: 'Deployment', ns: 'default' } }),
  ].map((n) => ({ ...n, ...overrides[n.id] }));
  return boot(
    initialState(),
    { nodes, edges, info: { demo: false, sourceTypes: [], version: '1.0.0' }, settings: {}, alerts: [], sources: [] },
    NOW,
  );
}
