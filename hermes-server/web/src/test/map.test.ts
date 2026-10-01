import { describe, expect, it } from 'vitest';
import { computeLinks, gridRef } from '../domain/map/links';
import { CELL_W, MAX_NETWORKS, layoutTopology, moreId, shapeSignature, topologyShape, HOST_PAD } from '../domain/map/layout';
import { Router } from '../domain/map/router';
import { reduce } from '../domain/reducer';
import { NOW, wireEdge, wireNode, world } from './fixtures';
import { parseEdge } from '../domain/model';
import { mountLinks, summary } from '../domain/selectors';
import { isGateway, networkIcon, networkMembers, networkSub } from '../domain/mapLabels';
import { networkTags } from '../domain/map/networks';
import { parseNode } from '../domain/model';

describe('the map layout', () => {
  it('puts hosts side by side in their cluster and pods in a grid under the host name', () => {
    const layout = layoutTopology(topologyShape(world(), null));
    const [c] = layout.clusters;
    expect(layout.clusters).toHaveLength(1);
    expect(c!.hosts.map((h) => h.id)).toEqual(['h1', 'h2']);
    const [h1, h2] = c!.hosts;
    expect(h2!.x).toBeGreaterThan(h1!.x + h1!.w); // side by side, with a gap
    expect(layout.items.get('w1')).toMatchObject({ x: h1!.x + (h1!.w - CELL_W) / 2 + CELL_W / 2 });
    expect(layout.w).toBeGreaterThanOrEqual(c!.w);
  });

  it('draws volumes after workloads and lays out three columns from five items on', () => {
    const nodes = [
      wireNode('c', 'cluster', null), wireNode('h', 'host', 'c'),
      wireNode('v', 'volume', 'h', { meta: { size: 10 } }),
      ...['a', 'b', 'c2', 'd', 'e'].map((id) => wireNode(id, 'workload', 'h')),
    ];
    const s = reduce(world(), { type: 'snapshot', nodes, edges: [] }, NOW).state;
    const shape = topologyShape(s, null);
    expect(shape[0]!.hosts[0]!.items.map((i) => i.id)).toEqual(['a', 'b', 'c2', 'd', 'e', 'v']);
    const host = layoutTopology(shape).clusters[0]!.hosts[0]!;
    expect(host.w).toBe(3 * CELL_W + 2 * HOST_PAD);
    expect(new Set(host.items.map((i) => i.x)).size).toBe(3);
  });

  it('a cluster with nothing reported is just its header', () => {
    const layout = layoutTopology(topologyShape(reduce(world(), { type: 'snapshot', nodes: [wireNode('c', 'cluster', null)], edges: [] }, NOW).state, null));
    expect(layout.clusters[0]!.hosts).toEqual([]);
  });

  it('wraps clusters into rows when they do not fit one', () => {
    const nodes = Array.from({ length: 6 }, (_, i) => [wireNode(`c${i}`, 'cluster', null), wireNode(`h${i}`, 'host', `c${i}`)]).flat();
    const layout = layoutTopology(topologyShape(reduce(world(), { type: 'snapshot', nodes, edges: [] }, NOW).state, null));
    const rows = new Set(layout.clusters.map((c) => c.y));
    expect(rows.size).toBeGreaterThan(1);
  });

  it('the signature changes with the shape and not with the numbers', () => {
    const a = world();
    const sameShape = reduce(a, { type: 'metrics', nodes: { w1: { cpu: 50 } }, edges: {} }, NOW).state;
    const other = reduce(a, { type: 'settings', settings: { showSystem: true } }, NOW).state;
    const sig = (s: typeof a) => shapeSignature(topologyShape(s, null));
    expect(sig(sameShape)).toBe(sig(a));
    expect(sig(other)).not.toBe(sig(a));
  });

  it('only the chosen clusters are drawn', () => {
    const nodes = [wireNode('c1', 'cluster', null), wireNode('c2', 'cluster', null)];
    const s = reduce(world(), { type: 'snapshot', nodes, edges: [] }, NOW).state;
    expect(topologyShape(s, new Set(['c2'])).map((c) => c.id)).toEqual(['c2']);
    expect(topologyShape(s, null).map((c) => c.id)).toEqual(['c1', 'c2']);
  });
});

describe('the link router', () => {
  it('routes around an obstacle with right angles only', () => {
    const router = new Router(600, 300);
    router.block(200, 60, 260, 240); // a wall between the two ports
    const r = router.find({ x: 60, y: 150, side: 1 }, { x: 460, y: 150, side: -1 });
    expect(r).not.toBeNull();
    const pts = r!.pts;
    for (let i = 1; i < pts.length; i++) expect(pts[i]![0] === pts[i - 1]![0] || pts[i]![1] === pts[i - 1]![1]).toBe(true);
    expect(Math.min(...pts.map((p) => p[1]))).toBeLessThan(60); // it went over the wall (or under it)
  });

  it('finds nothing when the way is walled off', () => {
    const router = new Router(600, 300);
    router.block(-60, 100, 700, 200);
    expect(router.find({ x: 60, y: 150, side: 1 }, { x: 460, y: 150, side: -1 })).toBeNull();
  });

  it('prefers a free corridor to one another link already uses', () => {
    const router = new Router(600, 300);
    const first = router.find({ x: 60, y: 150, side: 1 }, { x: 460, y: 150, side: -1 })!;
    router.commit(first.pts);
    const second = router.find({ x: 60, y: 150, side: 1 }, { x: 460, y: 150, side: -1 })!;
    expect(second.cost).toBeGreaterThan(first.cost);
  });
});

describe('links', () => {
  it('routes traffic between two pods and draws control links as a U under the hosts', () => {
    const s = world({}, [wireEdge('e1', 'w1', 'w3'), wireEdge('ctl', 'h1', 'h2', { type: 'control' }), wireEdge('lost', 'w1', 'ghost')]);
    const links = computeLinks(layoutTopology(topologyShape(s, null)), s.edges);
    expect(links.map((l) => l.edgeId)).toEqual(['e1', 'ctl']); // a link to something that is not drawn is left out
    expect(links[0]!.d).toMatch(/^M[\d. -]+(L[\d. -]+)+$/);
    expect(links[1]!.d).toMatch(/^M[\d.]+ [\d.]+ V[\d.]+ H[\d.]+ V[\d.]+$/);
    expect(links[1]!.end).toMatchObject({ dx: 0, dy: -1 });
  });

  it('names a place on the grid like a map', () => {
    expect([gridRef(0, 0), gridRef(-80, -80), gridRef(200, 400), gridRef(4000, 0)]).toEqual(['A01', 'A01', 'B04', 'Z01']);
  });
});

describe('networks', () => {
  const net = (id: string, extra: Record<string, unknown> = {}) => wireNode(id, 'network', 'c', { meta: { netKind: 'overlay', subnet: '10.0.2.0/24', members: 2, ...extra } });
  const withNet = (edges = [wireEdge('r1', 'n1', 'w1', { type: 'route', mbps: 0, base: 0 }), wireEdge('r2', 'n1', 'w3', { type: 'route', mbps: 0, base: 0 })]) => {
    const base = world();
    return reduce(base, { type: 'snapshot', nodes: [...base.nodes.values(), net('n1')].map((n) => ({ ...n, stale: false })) as never, edges }, NOW).state;
  };

  it('sit in a row above the hosts of their cluster, with a bus line each, and the hosts move down to make room', () => {
    const without = layoutTopology(topologyShape(world(), null)).clusters[0]!;
    const layout = layoutTopology(topologyShape(withNet(), null));
    const c = layout.clusters[0]!;
    const [n1] = c.networks;
    expect(c.networks.map((n) => n.id)).toEqual(['n1']);
    expect(layout.items.has('n1')).toBe(true);
    expect(n1!.bus).toBeGreaterThan(n1!.y + 67); // under the plate and its labels
    expect(n1!.bus).toBeLessThan(c.hosts[0]!.y);
    expect(n1!.trunk).toBeGreaterThan(n1!.x);
    expect(c.hosts[0]!.y).toBeGreaterThan(without.hosts[0]!.y);
    expect(c.h).toBeGreaterThan(without.h);
  });

  it('wrap to the width of the cluster, and each has a bus line of its own', () => {
    const base = world();
    const nodes = [...base.nodes.values(), ...['n1', 'n2', 'n3', 'n4', 'n5', 'n6', 'n7', 'n8'].map((id) => net(id))];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges: [] }, NOW).state;
    const c = layoutTopology(topologyShape(s, null)).clusters[0]!;
    expect(new Set(c.networks.map((n) => n.y)).size).toBeGreaterThan(1);
    expect(new Set(c.networks.map((n) => `${n.x},${n.y}`)).size).toBe(8);
    expect(new Set(c.networks.map((n) => n.bus)).size).toBe(8);
    expect(new Set(c.networks.map((n) => n.trunk)).size).toBe(8);
    expect(c.hosts[0]!.y).toBeGreaterThan(Math.max(...c.networks.map((n) => n.bus)));
  });

  it('run from the plate down to the bus, along it, and down the free corridor beside each member into its side', () => {
    const s = withNet();
    const layout = layoutTopology(topologyShape(s, null));
    const links = computeLinks(layout, s.edges);
    expect(links.map((l) => l.edgeId)).toEqual(['r1', 'r2']);
    const w1 = layout.items.get('w1')!;
    expect(links[0]!.d).toMatch(/^M[\d.]+ [\d.]+ H[\d.]+ V[\d.]+ H[\d.]+ V[\d.]+ H[\d.]+$/);
    expect(links[0]!.end).toMatchObject({ x: w1.x - 27, y: w1.y, dx: 1, dy: 0 });
    expect(summary(s)).toMatchObject({ trafficKnown: false, traffic: 0 });
  });

  it('a volume is joined to the workloads of its host that mount it, whether they are named as pods or as a Swarm service', () => {
    const base = world();
    const nodes = [
      ...base.nodes.values(),
      wireNode('v1', 'volume', 'h1', { meta: { size: 2, mountedBy: 'w1' } }),
      wireNode('v2', 'volume', 'h2', { meta: { mountedBy: 'demo_writer' } }),
      wireNode('t1', 'workload', 'h2', { name: 'demo_writer.1' }),
      wireNode('t2', 'workload', 'h1', { name: 'demo_writer.2' }),
    ];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges: [] }, NOW).state;
    expect(mountLinks(s).map((e) => e.id)).toEqual(['mount:w1>v1', 'mount:t1>v2']);
  });

  it('a route is a kind of link of its own, and an unknown kind is traffic', () => {
    expect(parseEdge(wireEdge('r', 'a', 'b', { type: 'route' })).type).toBe('route');
    expect(parseEdge(wireEdge('r', 'a', 'b', { type: 'whatever' })).type).toBe('traffic');
    expect(parseNode(net('n1')).kind).toBe('network');
  });

  it('lead to other networks: outside to an Ingress to a Service to its pods', () => {
    const base = world();
    const nodes = [...base.nodes.values(), net('out', { netKind: 'outside' }), net('ing', { netKind: 'ingress' }), net('svc', { netKind: 'service' })];
    const edges = [wireEdge('a', 'out', 'ing', { type: 'route' }), wireEdge('b', 'ing', 'svc', { type: 'route' }), wireEdge('c', 'svc', 'w1', { type: 'route' })];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges }, NOW).state;
    const layout = layoutTopology(topologyShape(s, null));
    const links = computeLinks(layout, s.edges);
    expect(links.map((l) => l.edgeId).sort()).toEqual(['a', 'b', 'c']);
    const ing = layout.items.get('ing')!;
    const a = links.find((l) => l.edgeId === 'a')!;
    expect(a.end).toMatchObject({ x: ing.x - 20, y: ing.y, dx: 1, dy: 0 }); // into the side of the Ingress plate
  });

  it('are drawn up to a cap, and the rest are one plate that stands for them', () => {
    const base = world();
    const many = Array.from({ length: 20 }, (_, i) => net(`n${i}`));
    const s = reduce(base, { type: 'snapshot', nodes: [...base.nodes.values(), ...many] as never, edges: [wireEdge('r', 'n0', 'w1', { type: 'route' }), wireEdge('gone', 'n19', 'w1', { type: 'route' })] }, NOW).state;
    const shape = topologyShape(s, null)[0]!;
    expect(shape.networks).toHaveLength(MAX_NETWORKS);
    expect(shape.networks.at(-1)).toBe(moreId('c'));
    expect(shape.networks[0]).toBe('n0');
    const layout = layoutTopology(topologyShape(s, null));
    expect(layout.items.has(moreId('c'))).toBe(true);
    expect(computeLinks(layout, s.edges).map((l) => l.edgeId)).toEqual(['r']); // a line to a network that is not drawn is left out
  });

  it('new kinds of network say what they are', () => {
    const p = parseNode(net('p', { netKind: 'policy', members: 3, addr: 'deny ingress' }));
    expect([isGateway(p), networkIcon(p), networkMembers(p)]).toEqual([true, 'shield', 'policy → 3']);
    expect(networkIcon(parseNode(net('g', { netKind: 'gateway' })))).toBe('gate');
    expect(networkIcon(parseNode(net('r', { netKind: 'httproute' })))).toBe('fanout');
  });

  it('run straight between neighbours in a row, and over the top of the plates they skip', () => {
    const base = world();
    const nodes = [...base.nodes.values(), net('na'), net('nb'), net('nc')];
    const edges = [wireEdge('ab', 'na', 'nb', { type: 'route' }), wireEdge('ac', 'na', 'nc', { type: 'route' })];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges }, NOW).state;
    const layout = layoutTopology(topologyShape(s, null));
    const links = computeLinks(layout, s.edges);
    const a = layout.items.get('na')!;
    expect(links.find((l) => l.edgeId === 'ab')!.d).toBe(`M${a.x + 20} ${a.y} H${layout.items.get('nb')!.x - 20}`);
    const over = links.find((l) => l.edgeId === 'ac')!;
    expect(over.d).toMatch(/^M[\d.]+ [\d.]+ H[\d.]+ V[\d.]+ H[\d.]+ V[\d.]+ H[\d.]+$/);
    expect(Number(/V(-?[\d.]+) H/.exec(over.d)![1])).toBeLessThan(a.y); // above the row
  });

  it('say what is behind a gateway and what is on a network', () => {
    const svc = parseNode(net('s', { netKind: 'service', members: 3, addr: '10.0.0.7:80' }));
    expect([isGateway(svc), networkSub(svc), networkMembers(svc), networkIcon(svc)]).toEqual([true, '10.0.0.7:80', 'service → 3', 'fanout']);
    expect(isGateway(parseNode(net('o')))).toBe(false);
  });

  it('a Kubernetes network of a system namespace is hidden like a system pod', () => {
    const base = world();
    const nodes = [...base.nodes.values(), net('n1', { ns: 'kube-system' })];
    const s = reduce(base, { type: 'snapshot', nodes: nodes as never, edges: [] }, NOW).state;
    expect(topologyShape(s, null)[0]!.networks).toEqual([]);
    const shown = reduce(s, { type: 'settings', settings: { showSystem: true } }, NOW).state;
    expect(topologyShape(shown, null)[0]!.networks).toEqual(['n1']);
  });

  it('say their range, and on the next line how many are on them', () => {
    expect([networkSub(parseNode(net('n1'))), networkMembers(parseNode(net('n1')))]).toEqual(['10.0.2.0/24', 'overlay · 2']);
    expect([networkSub(parseNode(net('n2', { subnet: undefined, members: 0 }))), networkMembers(parseNode(net('n2', { members: 0 }))), networkIcon(parseNode(net('n2')))]).toEqual(['overlay', 'overlay', 'net']);
  });

  it('give each network a colour of its own', () => {
    const s = withNet();
    const tags = networkTags(s);
    expect([...tags.colors.keys()]).toEqual(['n1']);
    expect(new Set(networkTags(reduce(s, { type: 'snapshot', nodes: [...s.nodes.values(), net('n2'), net('n3')] as never, edges: [] }, NOW).state).colors.values()).size).toBe(3);
  });
});
