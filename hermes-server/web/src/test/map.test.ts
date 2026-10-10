import { describe, expect, it } from 'vitest';
import { assign, regionOf } from '../domain/regions';
import { computeLinks, gridRef } from '../domain/map/links';
import { CELL_W, MAX_COLS, MAX_NETWORKS, autoCols, layoutToFit, layoutTopology, moreId, shapeSignature, topologyShape, HOST_PAD } from '../domain/map/layout';
import { Router } from '../domain/map/router';
import { arrange, dropSide, rowsOf } from '../domain/map/order';
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

describe('resizing a host box', () => {
  const eight = () => topologyShape(reduce(world(), { type: 'snapshot', nodes: [
    wireNode('c', 'cluster', null), wireNode('h', 'host', 'c'),
    ...['a', 'b', 'c2', 'd', 'e', 'f', 'g', 'i'].map((id) => wireNode(id, 'workload', 'h')),
  ], edges: [] }, NOW).state, null);

  it('lays the workloads out in the columns a person chose, and the host stays what it was without a choice', () => {
    const auto = layoutTopology(eight()).hosts.get('h')!;
    expect(auto.cols).toBe(autoCols(8));
    const wide = layoutTopology(eight(), new Map([['h', 6]])).hosts.get('h')!;
    expect(wide.cols).toBe(6);
    expect(wide.w).toBeGreaterThan(auto.w); // wider
    expect(wide.h).toBeLessThan(auto.h); // and so flatter
    const narrow = layoutTopology(eight(), new Map([['h', 1]])).hosts.get('h')!;
    expect(narrow.h).toBeGreaterThan(auto.h);
    expect(narrow.items.every((it) => it.x === narrow.items[0]!.x)).toBe(true); // one column
  });

  it('never gives a host more columns than it has workloads or than the limit, nor fewer than one', () => {
    expect(layoutTopology(eight(), new Map([['h', 99]])).hosts.get('h')!.cols).toBe(Math.min(MAX_COLS, 8));
    expect(layoutTopology(eight(), new Map([['h', 0]])).hosts.get('h')!.cols).toBe(autoCols(8)); // 0 is no choice
    expect(layoutTopology(eight(), new Map([['h', -3]])).hosts.get('h')!.cols).toBe(1);
  });
});

describe('arranging the clusters for the screen', () => {
  const six = Array.from({ length: 6 }, (_, i) => ({
    id: `c${i}`, networks: [], hasControl: false,
    hosts: [{ id: `h${i}`, items: ['a', 'b', 'c', 'd', 'e', 'f'].map((id) => ({ id: `${i}${id}`, volume: false })) }],
  }));

  it('puts the clusters in wide rows on a landscape screen and in a column on a portrait one', () => {
    const wide = layoutToFit(six, new Map(), { w: 3600, h: 900 });
    const tall = layoutToFit(six, new Map(), { w: 900, h: 3600 });
    expect(wide.w / wide.h).toBeGreaterThan(tall.w / tall.h);
    expect(wide.w).toBeGreaterThan(tall.w);
  });

  it('shows the map larger than the fixed row width does on a wide screen', () => {
    const view = { w: 3600, h: 900 };
    const fixed = layoutTopology(six);
    const fit = layoutToFit(six, new Map(), view);
    const scale = (l: { w: number; h: number }) => Math.min(view.w / l.w, view.h / l.h);
    expect(scale(fit)).toBeGreaterThan(scale(fixed));
  });

  it('is the plain layout when the size of the screen is not known', () => {
    expect(layoutToFit(six, new Map(), null)).toEqual(layoutTopology(six));
    expect(layoutToFit(six, new Map(), { w: 0, h: 0 })).toEqual(layoutTopology(six));
  });
});

describe('arranging the clusters by hand', () => {
  const box = (id: string, x: number, y: number) => ({ id, x, y, w: 100, h: 50 });

  it('reads the lines the clusters are on from where they are drawn', () => {
    expect(rowsOf([box('c', 0, 100), box('b', 150, 0), box('a', 0, 0)])).toEqual([['a', 'b'], ['c']]);
  });

  it('puts a cluster on the line of the one it is dropped on, left or right of it, or on a line of its own above or below it', () => {
    const rows = [['a', 'b'], ['c']];
    expect(arrange(rows, 'c', 'b', 'r')).toEqual([['a', 'b', 'c']]); // the line it left is gone
    expect(arrange(rows, 'c', 'a', 'l')).toEqual([['c', 'a', 'b']]);
    expect(arrange(rows, 'a', 'c', 'b')).toEqual([['b'], ['c'], ['a']]);
    expect(arrange(rows, 'c', 'a', 't')).toEqual([['c'], ['a', 'b']]);
    expect(arrange(rows, 'a', 'a', 'r')).toEqual(rows);
    expect(arrange(rows, 'x', 'a', 'r')).toEqual(rows);
  });

  it('says which side of a cluster a drop is on from where the pointer is in it', () => {
    const b = { x: 100, y: 100, w: 400, h: 200 };
    expect(dropSide(b, 450, 200)).toBe('r');
    expect(dropSide(b, 150, 200)).toBe('l');
    expect(dropSide(b, 300, 290)).toBe('b');
    expect(dropSide(b, 300, 110)).toBe('t');
  });

  const three = ['a', 'b', 'c'].map((id) => ({
    id, networks: [], hasControl: false,
    hosts: [{ id: `h${id}`, items: ['x', 'y', 'z', 'w', 'v', 'u'].map((i) => ({ id: `${id}${i}`, volume: false })) }],
  }));

  it('keeps the lines a person chose, whatever the screen: all three on one line, though the auto layout would wrap them', () => {
    expect(new Set(layoutTopology(three, new Map(), 700).clusters.map((c) => c.y)).size).toBeGreaterThan(1); // wraps by itself
    const one = layoutToFit(three, new Map(), { w: 800, h: 2000 }, [['a', 'b', 'c']]);
    expect(new Set(one.clusters.map((c) => c.y)).size).toBe(1);
    expect(one.clusters.map((c) => c.id)).toEqual(['a', 'b', 'c']);
  });

  it('puts a source nobody placed yet on a line of its own at the end, and forgets ids that are gone', () => {
    const l = layoutToFit(three, new Map(), null, [['b', 'gone'], ['a']]);
    expect(rowsOf(l.clusters)).toEqual([['b'], ['a'], ['c']]);
  });
});

describe('regions on the map', () => {
  const shape = ['a', 'b', 'c'].map((id) => ({ id, networks: [], hosts: [{ id: `${id}-h`, items: [{ id: `${id}-w`, volume: false }] }], hasControl: false }));
  const region = { id: 'r1', name: 'Office', color: '#4aa3ff', clusterIds: ['a', 'b'] };

  it('frames the clusters of a region and keeps the others outside the frame', () => {
    const l = layoutToFit(shape, new Map(), null, undefined, [region]);
    const [frame] = l.regions;
    expect(l.regions).toHaveLength(1);
    const inside = (id: string) => { const c = l.clusters.find((x) => x.id === id)!; return c.x >= frame!.x && c.y >= frame!.y && c.x + c.w <= frame!.x + frame!.w && c.y + c.h <= frame!.y + frame!.h; };
    expect(inside('a') && inside('b')).toBe(true);
    expect(inside('c')).toBe(false);
  });

  it('moves the items with their cluster, so the links still land on them', () => {
    const l = layoutToFit(shape, new Map(), null, undefined, [region]);
    const host = l.clusters.find((c) => c.id === 'b')!.hosts[0]!;
    expect(l.hosts.get('b-h')).toBe(host);
    expect(l.items.get('b-w')!.y).toBeGreaterThan(host.y);
    expect(l.items.get('b-w')!.x).toBeGreaterThan(host.x);
  });

  it('draws no frame for a region whose clusters are gone', () => {
    expect(layoutToFit(shape, new Map(), null, undefined, [{ ...region, clusterIds: ['zzz'] }]).regions).toEqual([]);
  });
});

describe('which region a cluster is in', () => {
  const r = (id: string, ids: string[]) => ({ id, name: id, color: '#fff', clusterIds: ids });
  it('moves a cluster from one region to another and drops the region it leaves empty', () => {
    expect(assign([r('x', ['a']), r('y', ['b'])], 'a', 'y')).toEqual([r('y', ['b', 'a'])]);
  });
  it('takes a cluster out of every region with null', () => {
    expect(assign([r('x', ['a', 'b'])], 'a', null)).toEqual([r('x', ['b'])]);
    expect(regionOf([r('x', ['a'])], 'a')).toBe('x');
    expect(regionOf([r('x', ['a'])], 'z')).toBeNull();
  });
});

describe('the lines of the regions', () => {
  const sh = ['a', 'b'].map((id) => ({ id, networks: [], hosts: [{ id: `${id}-h`, items: [{ id: `${id}-w`, volume: false }] }], hasControl: false }));
  const regions = [{ id: 'r1', name: 'One', color: '#fff', clusterIds: ['a'] }, { id: 'r2', name: 'Two', color: '#fff', clusterIds: ['b'] }];
  const at = (l: ReturnType<typeof layoutToFit>, id: string) => l.blocks.find((b) => b.id === id)!;

  it('puts two regions side by side, or one under the other when a person arranged them so', () => {
    const side = layoutToFit(sh, new Map(), null, undefined, regions, [['r2', 'r1']]);
    expect(at(side, 'r2').x).toBeLessThan(at(side, 'r1').x);
    expect(at(side, 'r2').y).toBe(at(side, 'r1').y);
    const stacked = layoutToFit(sh, new Map(), null, undefined, regions, [['r1'], ['r2']]);
    expect(at(stacked, 'r2').y).toBeGreaterThan(at(stacked, 'r1').y + at(stacked, 'r1').h - 1);
    expect(at(stacked, 'r2').x).toBe(at(stacked, 'r1').x);
  });
});
