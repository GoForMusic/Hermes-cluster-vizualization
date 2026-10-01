import { describe, expect, it } from 'vitest';
import type { Severity } from '../generated/Severity';
import { reduce } from '../domain/reducer';
import { activeAlerts, clusterOf, edgeBroken, effectiveSeverity, FLOWS_FRESH_MS, hiddenSystemCount, incidentsFor, isShown, isSystem, shownKids, summary, topFlows, volumeTotals } from '../domain/selectors';
import { NOW, wireEdge, wireNode, world } from './fixtures';

describe('selectors', () => {
  it('finds the cluster of anything below it', () => {
    const s = world();
    expect(['w1', 'h2', 'c'].map((id) => clusterOf(s, id)?.id)).toEqual(['c', 'c', 'c']);
    expect(clusterOf(s, 'ghost')).toBeUndefined();
  });

  it('hides system pods until asked, but never one that is failing', () => {
    let s = world();
    expect([isSystem(s, s.nodes.get('w2')!), isSystem(s, s.nodes.get('w1')!)]).toEqual([true, false]);
    expect(shownKids(s, 'h1').map((n) => n.id)).toEqual(['w1']);
    expect(hiddenSystemCount(s)).toBe(1);
    s = reduce(s, { type: 'status', id: 'w2', own: 'warn', reason: '' }, NOW).state;
    expect(isShown(s, s.nodes.get('w2')!)).toBe(true);
    expect(hiddenSystemCount(s)).toBe(0);
    s = reduce(world(), { type: 'settings', settings: { showSystem: true } }, NOW).state;
    expect(shownKids(s, 'h1').map((n) => n.id)).toEqual(['w1', 'w2']);
  });

  it('counts hosts and workloads that are running, without the hidden system pods', () => {
    const s = reduce(world(), { type: 'status', id: 'h2', own: 'crit', reason: '' }, NOW).state;
    expect(summary(s)).toMatchObject({ clusters: 1, hostsUp: 1, hostsTotal: 2, wlRun: 1, wlTotal: 2 });
  });

  it('measures traffic at the sender when the agents do, and says it is unknown when nothing does', () => {
    expect(summary(world())).toMatchObject({ trafficKnown: false, traffic: 0 });
    const measured = reduce(world(), { type: 'metrics', nodes: { w1: { txMbps: 3 }, w3: { txMbps: 4 } }, edges: {} }, NOW).state;
    expect(summary(measured)).toMatchObject({ trafficKnown: true, traffic: 7 });
    const drawn = world({}, [wireEdge('e1', 'w1', 'w3', { mbps: 5 }), wireEdge('ctl', 'h1', 'h2', { type: 'control', mbps: 9 })]);
    expect(summary(drawn)).toMatchObject({ trafficKnown: true, traffic: 5 });
  });

  it('a link is broken when either end is down or unknown', () => {
    const e = wireEdge('e1', 'w1', 'w3');
    const s = world({}, [e]);
    expect(edgeBroken(s, s.edges[0]!)).toBe(false);
    const down = reduce(s, { type: 'status', id: 'h2', own: 'crit', reason: '' }, NOW).state;
    expect(edgeBroken(down, down.edges[0]!)).toBe(true);
    expect(edgeBroken(s, { ...s.edges[0]!, to: 'ghost' })).toBe(true);
  });

  it('separates active alerts from resolved ones and counts them by severity', () => {
    let s = world();
    const a = (id: number, sev: Severity, resolvedTs: number | null) => ({ id, key: `${id}`, sev, nodeId: 'w1', title: '', detail: '', ts: id, resolvedTs, ack: false, snapshot: '' });
    for (const alert of [a(1, 'crit', null), a(2, 'warn', null), a(3, 'crit', 99)]) s = reduce(s, { type: 'alert', alert, isNew: true }, NOW).state;
    expect(activeAlerts(s).map((x) => x.id).sort()).toEqual([1, 2]);
    expect(summary(s)).toMatchObject({ crit: 1, warn: 1 });
  });

  it("an incident on a host also explains its workloads' red cells, but not another host's", () => {
    let s = world();
    const alert = (id: number, nodeId: string, ts: number, resolvedTs: number | null) => ({ id, key: `${id}`, sev: 'crit' as const, nodeId, title: `${nodeId} down`, detail: '', ts, resolvedTs, ack: false, snapshot: '' });
    s = reduce(s, { type: 'alert', alert: alert(1, 'h2', NOW - 1000, NOW - 500), isNew: true }, NOW).state; // w3's host went down briefly
    s = reduce(s, { type: 'alert', alert: alert(2, 'h1', NOW - 1000, NOW - 500), isNew: true }, NOW).state; // w1's host, unrelated to w3
    expect(incidentsFor(s, 'w3', NOW - 10_000, NOW).map((a) => a.id)).toEqual([1]);
    expect(incidentsFor(s, 'w1', NOW - 10_000, NOW).map((a) => a.id)).toEqual([2]);
    expect(incidentsFor(s, 'h2', NOW - 10_000, NOW).map((a) => a.id)).toEqual([1]);
    expect(incidentsFor(s, 'w3', NOW - 100, NOW)).toEqual([]); // resolved well before the window opened
  });

  it("a host's amber cell is explained by a struggling workload's own incident, not just its ancestors'", () => {
    let s = world();
    const alert = (id: number, nodeId: string) => ({ id, key: `${id}`, sev: 'crit' as const, nodeId, title: `${nodeId} down`, detail: '', ts: NOW - 1000, resolvedTs: null, ack: false, snapshot: '' });
    s = reduce(s, { type: 'alert', alert: alert(1, 'w1'), isNew: true }, NOW).state; // w1's host h1 shows amber for this
    expect(incidentsFor(s, 'h1', NOW - 10_000, NOW).map((a) => a.id)).toEqual([1]);
    expect(incidentsFor(s, 'h2', NOW - 10_000, NOW)).toEqual([]); // a different host, unrelated
  });

  it("caps a crit inherited from a descendant at warn — it cannot make the host itself look down — but never one on the node itself or inherited from an ancestor", () => {
    let s = world();
    const alert = (id: number, nodeId: string) => ({ id, key: `${id}`, sev: 'crit' as const, nodeId, title: '', detail: '', ts: NOW, resolvedTs: null, ack: false, snapshot: '' });
    const onW1 = alert(1, 'w1');
    const onH1 = alert(2, 'h1');
    expect(effectiveSeverity(s, 'h1', onW1)).toBe('warn'); // w1 is h1's child: capped
    expect(effectiveSeverity(s, 'w1', onW1)).toBe('crit'); // on the node itself: unchanged
    s = reduce(s, { type: 'alert', alert: onH1, isNew: true }, NOW).state;
    expect(effectiveSeverity(s, 'w1', onH1)).toBe('crit'); // inherited from its host, a real outage: unchanged
  });

  const line = (mbps: number, src = 'a') => ({ src, dst: 'b', via: '', port: 80, mbps, external: false });

  it('lists the busiest connections of every source, and forgets the ones whose agent went quiet', () => {
    let s = world();
    s = reduce(s, { type: 'flows', source: 's1', flows: [line(5), line(50, 'x')] }, NOW).state;
    s = reduce(s, { type: 'flows', source: 's2', flows: [line(20, 'y')] }, NOW + 20_000).state;
    expect(topFlows(s, NOW + 21_000).map((f) => f.mbps)).toEqual([50, 20, 5]);
    expect(topFlows(s, NOW + 21_000, 2)).toHaveLength(2);
    expect(topFlows(s, NOW + FLOWS_FRESH_MS + 1).map((f) => f.mbps)).toEqual([20]); // s1 said nothing for 30 s
    expect(topFlows(s, NOW + 2 * FLOWS_FRESH_MS)).toEqual([]);
  });

  it('a new list of a source replaces its old one', () => {
    let s = reduce(world(), { type: 'flows', source: 's1', flows: [line(5), line(6, 'q')] }, NOW).state;
    s = reduce(s, { type: 'flows', source: 's1', flows: [line(1)] }, NOW + 5000).state;
    expect(topFlows(s, NOW + 6000).map((f) => f.mbps)).toEqual([1]);
  });

  it('adds up the volumes: how much is used, how much is provisioned, and how many have no size', () => {
    const base = world();
    const nodes = [...base.nodes.values(), wireNode('v1', 'volume', 'h1', { m: { used: 1 }, meta: { size: 4 } }), wireNode('v2', 'volume', 'h2', { m: { used: 0.5 } })];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges: [] }, NOW).state;
    expect(volumeTotals(s)).toEqual({ count: 2, used: 1.5, size: 4, sized: 1 });
    expect(volumeTotals(base)).toEqual({ count: 0, used: 0, size: 0, sized: 0 });
  });
});
