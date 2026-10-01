import { act, fireEvent, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { renderWithHub } from './render';
import { TopologyMap } from '../features/topology/TopologyMap';

describe('the map, with networks', () => {
  const withNetwork = (mbps: number) => ({
    type: 'snapshot' as const,
    nodes: [
      { id: 'c', kind: 'cluster', name: 'c', parent: null, provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta: {} },
      { id: 'h', kind: 'host', name: 'h', parent: 'c', provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta: { ip: '10.0.0.1', role: 'worker' } },
      { id: 'w', kind: 'workload', name: 'w', parent: 'h', provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta: {} },
      { id: 'svc', kind: 'network', name: 'web', parent: 'c', provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta: { netKind: 'service', members: 1 } },
    ],
    edges: [{ id: 'svc>w', from: 'svc', to: 'w', base: 0, mbps, type: 'route' }],
  });

  it('says only that there is a path when nothing measured the traffic on a route, and the rate when something did', async () => {
    const { store, container } = await renderWithHub(<TopologyMap />);
    act(() => store.apply(withNetwork(0)));
    expect(container.querySelectorAll('.gedge.route')).toHaveLength(1);
    expect(container.querySelector('.gedge.route .link-label')!.textContent).toBe('');
    expect(container.querySelectorAll('.gedge.route .particle')).toHaveLength(0);
    act(() => store.apply({ type: 'metrics', nodes: {}, edges: { 'svc>w': 12.4 } }));
    expect(container.querySelector('.gedge.route .link-label')!.textContent).toBe('12 Mb/s');
    expect(container.querySelectorAll('.gedge.route .particle').length).toBeGreaterThan(0);
  });
});

describe('the map, with an ingress', () => {
  const node = (id: string, kind: string, parent: string | null, meta: Record<string, unknown> = {}) => ({ id, kind, name: id, parent, provider: 'kubernetes', own: 'ok', status: 'ok', reason: '', since: 1, m: {}, meta });
  const snapshot = {
    type: 'snapshot' as const,
    nodes: [node('c', 'cluster', null), node('h', 'host', 'c', { ip: '10.0.0.1', role: 'worker' }), node('w', 'workload', 'h'), node('out', 'network', 'c', { netKind: 'outside' }), node('ing', 'network', 'c', { netKind: 'ingress' }), node('svc', 'network', 'c', { netKind: 'service' })],
    edges: [
      { id: 'out>ing', from: 'out', to: 'ing', base: 0, mbps: 0, type: 'route' },
      { id: 'ing>svc', from: 'ing', to: 'svc', base: 0, mbps: 0, type: 'route' },
      { id: 'svc>w', from: 'svc', to: 'w', base: 0, mbps: 0, type: 'route' },
    ],
  };

  it('marks the rates it deduced (through the Ingress) as approximate, and the ones it measured as they are', async () => {
    const { store, container } = await renderWithHub(<TopologyMap />);
    act(() => store.apply(snapshot));
    act(() => store.apply({ type: 'metrics', nodes: {}, edges: { 'out>ing': 8, 'ing>svc': 8, 'svc>w': 8 } }));
    const labels = [...container.querySelectorAll('.gedge.route')].map((g) => g.querySelector('.link-label')!.textContent);
    expect(labels.sort()).toEqual(['8.0 Mb/s', '≈ 8.0 Mb/s', '≈ 8.0 Mb/s']);
  });
});

describe('the map', () => {
  it('draws the cluster, its host and the workload, with what each says about itself', async () => {
    await renderWithHub(<TopologyMap />);
    expect(screen.getByText('c')).toBeInTheDocument(); // the cluster's name
    expect(screen.getAllByText('h').length).toBeGreaterThan(0);
    expect(screen.getByText('w')).toBeInTheDocument();
    expect(screen.getByText('running')).toBeInTheDocument(); // a workload that measures nothing says it runs
  });

  it('follows the live state: a failing workload changes its symbol and its words', async () => {
    const { store, container } = await renderWithHub(<TopologyMap />);
    expect(container.querySelector('.gnode.st-ok')).not.toBeNull();
    act(() => store.apply({ type: 'status', id: 'w', own: 'crit', reason: 'CrashLoopBackOff' }));
    expect(container.querySelector('.gnode.st-crit')).not.toBeNull();
    expect(screen.getByText('CrashLoopBackOff')).toBeInTheDocument();
  });

  it('a click selects a node when the map is interactive, and another click on it clears the selection', async () => {
    const onSelect = vi.fn();
    const { container, rerender } = await renderWithHub(<TopologyMap interactive onSelect={onSelect} selectedId={null} />);
    fireEvent.click(container.querySelector('.gnode')!);
    expect(onSelect).toHaveBeenLastCalledWith('w');
    void rerender;
  });

  it('is only a picture when it is not interactive', async () => {
    const onSelect = vi.fn();
    const { container } = await renderWithHub(<TopologyMap onSelect={onSelect} />);
    fireEvent.click(container.querySelector('.gnode')!);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it('draws only the clusters it was asked for', async () => {
    const { container } = await renderWithHub(<TopologyMap visible={new Set(['nothing'])} />);
    expect(container.querySelector('.cluster-box')).toBeNull();
  });
});
