import { describe, expect, it } from 'vitest';
import type { Alert } from '../domain/model';
import { reduce } from '../domain/reducer';
import { NOW, wireEdge, wireNode, world } from './fixtures';

const alert = (id: number, ts: number, extra: Partial<Alert> = {}): Alert => ({ id, key: `k${id}`, sev: 'crit', nodeId: 'w1', title: `alert ${id}`, detail: '', ts, resolvedTs: null, ack: false, snapshot: '', ...extra });

describe('events', () => {
  it('a status change updates the node and what depends on it, and leaves the old state alone', () => {
    const before = world();
    const { state } = reduce(before, { type: 'status', id: 'h1', own: 'crit', reason: 'NodeNotReady' }, NOW + 5);
    expect(state.nodes.get('h1')).toMatchObject({ own: 'crit', status: 'crit', reason: 'NodeNotReady' });
    expect(state.nodes.get('w1')!.status).toBe('unknown');
    expect(state.nodes.get('c')!.status).toBe('warn'); // one host down, h2 still fine: a partial outage, not the whole cluster
    expect(before.nodes.get('h1')!.own).toBe('ok');
  });

  it('a status of a node nobody knows, or a state nobody knows, changes nothing', () => {
    const s = world();
    expect(reduce(s, { type: 'status', id: 'ghost', own: 'crit', reason: '' }, NOW).state).toBe(s);
    expect(reduce(s, { type: 'status', id: 'w1', own: 'exploded', reason: '' }, NOW).state).toBe(s);
  });

  it('a meta patch is merged into what the node already says', () => {
    const { state } = reduce(world(), { type: 'meta', id: 'w1', meta: { restarts: 4 } }, NOW);
    expect(state.nodes.get('w1')!.meta).toEqual({ type: 'Deployment', ns: 'default', restarts: 4 });
  });

  it('metrics are merged into the nodes and the links, and feed the short series', () => {
    const s = world({}, [wireEdge('e1', 'w1', 'w3')]);
    const { state } = reduce(s, { type: 'metrics', nodes: { w1: { cpu: 12, memMiB: 100 }, ghost: { cpu: 1 } }, edges: { e1: 7.5, nope: 1 } }, NOW);
    expect(state.nodes.get('w1')!.m).toEqual({ cpu: 12, memMiB: 100 });
    expect(state.edges[0]!.mbps).toBe(7.5);
    expect([state.history.get('cpu:w1'), state.history.get('net:e1'), state.history.get('net:total')]).toEqual([[12], [7.5], [7.5]]);
    expect(state.nodes.has('ghost')).toBe(false);
  });

  it('the series keep the last sixty samples', () => {
    let s = world();
    for (let i = 0; i < 70; i++) s = reduce(s, { type: 'metrics', nodes: { w1: { cpu: i } }, edges: {} }, NOW).state;
    const cpu = s.history.get('cpu:w1')!;
    expect([cpu.length, cpu[0], cpu.at(-1)]).toEqual([60, 10, 69]);
  });

  it('a snapshot replaces the topology and keeps what is not part of it', () => {
    const s = reduce(world(), { type: 'status', id: 'h1', own: 'crit', reason: '' }, NOW).state;
    const fresh = reduce(s, { type: 'snapshot', nodes: [wireNode('c2', 'cluster', null), wireNode('h9', 'host', 'c2')], edges: [] }, NOW).state;
    expect([...fresh.nodes.keys()]).toEqual(['c2', 'h9']);
    expect(fresh.kids.get('c2')).toEqual(['h9']);
    expect(fresh.settings).toBe(s.settings);
  });

  it('an alert is added, replaced in place when it changes, kept newest first, and announced only when it is new', () => {
    let s = world();
    const r1 = reduce(s, { type: 'alert', alert: alert(1, 100), isNew: true }, NOW);
    expect(r1.newAlert?.id).toBe(1);
    s = reduce(r1.state, { type: 'alert', alert: alert(2, 200), isNew: true }, NOW).state;
    const r3 = reduce(s, { type: 'alert', alert: alert(1, 100, { resolvedTs: 150 }), isNew: false }, NOW);
    expect(r3.newAlert).toBeUndefined();
    expect(r3.state.alerts.map((a) => [a.id, a.resolvedTs])).toEqual([[2, null], [1, 150]]);
  });

  it('settings change what is shown: a rule threshold moves a volume from ok to warning', () => {
    const s = world();
    const next = reduce(s, { type: 'settings', settings: { rotateSec: 30, rules: [{ id: 'volume-usage', value: 10, crit: 20 }] } }, NOW).state;
    expect(next.settings.rotateSec).toBe(30);
    expect(next.settings.rules.find((r) => r.id === 'volume-usage')).toMatchObject({ value: 10, crit: 20, enabled: true });
  });

  it('a live settings change moves an unpinned session to the new default range, but never one that has picked its own', () => {
    const unpinned = world();
    const moved = reduce(unpinned, { type: 'settings', settings: { defaultRange: '1h' } }, NOW).state;
    expect(moved.range.id).toBe('1h');
    expect(moved.rangeAuto).toBe(false);

    const pinned = { ...world(), rangePinned: true };
    const kept = reduce(pinned, { type: 'settings', settings: { defaultRange: '1h' } }, NOW).state;
    expect(kept.range).toBe(pinned.range);
    expect(kept.rangeAuto).toBe(pinned.rangeAuto);
  });

  it('a change of the list of sources asks for it to be fetched again', () => {
    const s = world();
    expect(reduce(s, { type: 'sources' }, NOW)).toEqual({ state: s, refreshSources: true });
  });
});
