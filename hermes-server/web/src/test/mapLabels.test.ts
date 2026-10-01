import { describe, expect, it } from 'vitest';
import { hasCapacity, volumeBadge, volumeFill, volumeSub } from '../domain/mapLabels';
import type { Node } from '../domain/model';
import { summary } from '../domain/selectors';
import { NOW, wireNode, world } from './fixtures';
import { boot } from '../domain/reducer';
import { initialState } from '../domain/hubState';

const volume = (used: number, size?: number): Node => ({
  id: 'v', kind: 'volume', name: 'v', parent: 'h', provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 0, stale: false,
  m: { used }, meta: size === undefined ? {} : { size },
});

describe('a volume with a size', () => {
  it('is a percentage of that size and says how much of it is used', () => {
    const v = volume(0.5, 2);
    expect([hasCapacity(v), volumeFill(v), volumeBadge(v), volumeSub(v)]).toEqual([true, 0.25, '25%', '0.5/2 GiB']);
  });
});

describe('a volume with no limit (a local Docker volume)', () => {
  it('says how much it takes, and has no percentage to show', () => {
    const v = volume(1.5);
    expect([hasCapacity(v), volumeFill(v), volumeBadge(v), volumeSub(v)]).toEqual([false, 0, '1.5G', '1.5 GiB used']);
    expect(volumeBadge(volume(0.25))).toBe('256M');
    expect(volumeBadge(volume(12))).toBe('12G');
  });

  it('has no percentage: the HUD shows the fullest volume that has a limit, not an average', () => {
    const wire = (id: string, used: number, size?: number) => wireNode(id, 'volume', 'h', { m: { used }, meta: size === undefined ? {} : { size } });
    const nodes = [wireNode('h', 'host', null), wire('a', 0.2, 10), wire('b', 1.9, 2), wire('c', 0.5, 10), wire('unlimited', 50)];
    const s = boot(initialState(), { nodes, edges: [], info: { demo: false, sourceTypes: [], version: '' }, settings: {}, alerts: [], sources: [] }, NOW);
    const sum = summary(s);
    expect(sum).toMatchObject({ volumes: 4, volumeUsed: 52.6 });
    expect(sum.fullest?.pct).toBeCloseTo(95);
    void world;
  });

  it('shows only what is used when no volume has a limit', () => {
    const v = wireNode('v', 'volume', 'h', { m: { used: 3 }, meta: {} });
    const s = boot(initialState(), { nodes: [wireNode('h', 'host', null), v], edges: [], info: { demo: false, sourceTypes: [], version: '' }, settings: {}, alerts: [], sources: [] }, NOW);
    expect(summary(s)).toMatchObject({ volumes: 1, fullest: null, volumeUsed: 3 });
  });

  it('never becomes a warning or an alert: nothing is known to be full', () => {
    const wireV = wireNode('v', 'volume', 'h', { m: { used: 500 }, meta: {} });
    const s = boot(initialState(), { nodes: [wireNode('h', 'host', null), wireV], edges: [], info: { demo: false, sourceTypes: [], version: '' }, settings: {}, alerts: [], sources: [] }, NOW);
    expect(s.nodes.get('v')!.own).toBe('ok');
  });
});
