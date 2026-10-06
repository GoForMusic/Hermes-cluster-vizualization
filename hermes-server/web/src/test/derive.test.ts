import { describe, expect, it } from 'vitest';
import { deriveStatuses } from '../domain/derive';
import { initialState } from '../domain/hubState';
import { boot } from '../domain/reducer';
import { NOW, wireNode, world } from './fixtures';

const status = (s: ReturnType<typeof world>, id: string) => s.nodes.get(id)!.status;

/** a host with one volume of 100 GiB, `used` of it full */
const withVolume = (used: number) =>
  boot(
    initialState(),
    { nodes: [wireNode('h', 'host', null), wireNode('v', 'volume', 'h', { m: { used }, meta: { size: 100 } })], edges: [], info: { demo: false, sourceTypes: [], version: '' }, settings: {}, alerts: [], sources: [] },
    NOW,
  ).nodes.get('v')!;

describe('the effective status', () => {
  it('is what the source reports when nothing else is known', () => {
    const s = world({ w1: { own: 'warn' }, w3: { own: 'crit' } });
    expect([status(s, 'w1'), status(s, 'w2'), status(s, 'w3')]).toEqual(['warn', 'ok', 'crit']);
  });

  it('makes everything on a crashed host unknown, but not the host itself', () => {
    const s = world({ h1: { own: 'crit' }, w1: { own: 'crit' } });
    expect(status(s, 'h1')).toBe('crit');
    expect([status(s, 'w1'), status(s, 'w2')]).toEqual(['unknown', 'unknown']);
    expect(status(s, 'w3')).toBe('ok');
  });

  it('makes the containers of a Docker machine unknown when its own agent goes quiet, but not the pods of a Kubernetes node', () => {
    const docker = world({ h1: { stale: true, provider: 'docker' }, w1: { provider: 'docker' }, w2: { provider: 'docker' } });
    expect([status(docker, 'w1'), status(docker, 'w2')]).toEqual(['unknown', 'unknown']);
    expect(status(docker, 'w3')).toBe('ok'); // another machine, still talking
    const kubernetes = world({ h1: { stale: true } });
    expect([status(kubernetes, 'w1'), status(kubernetes, 'w2')]).toEqual(['ok', 'ok']); // the API still vouches for them
  });

  it('makes what an unreachable source reported unknown: the last known state is not the current one', () => {
    const s = world({ h1: { stale: true }, h2: { stale: true }, w1: { stale: true, own: 'warn' }, w3: { stale: true } });
    expect([status(s, 'h1'), status(s, 'h2'), status(s, 'w1'), status(s, 'w3')]).toEqual(['unknown', 'unknown', 'unknown', 'unknown']);
  });

  it('keeps a failure it last saw: something seen down does not become unknown just because the source went quiet', () => {
    const s = world({ h1: { stale: true, own: 'crit' }, w3: { stale: true, own: 'crit' } });
    expect([status(s, 'h1'), status(s, 'w3')]).toEqual(['crit', 'crit']);
  });

  it('gives a cluster the worst status of what is under it, ignoring what is unknown', () => {
    expect(status(world(), 'c')).toBe('ok');
    expect(status(world({ w3: { own: 'warn' } }), 'c')).toBe('warn');
    expect(status(world({ w3: { own: 'warn' }, w1: { own: 'crit' } }), 'c')).toBe('crit');
  });

  it('shows a host amber, never red, for a struggling workload: "down" stays for its own heartbeat', () => {
    const s = world({ w1: { own: 'warn' } }); // a pod stuck pending, say
    expect(status(s, 'h1')).toBe('warn');
    const crashed = world({ w1: { own: 'crit' } }); // fully crashed: still just this host's own workload, not its heartbeat
    expect(status(crashed, 'h1')).toBe('warn');
  });

  it('calls one dead host among live ones a partial outage (amber), and every host down the real thing (red)', () => {
    expect(status(world({ h1: { own: 'crit' } }), 'c')).toBe('warn');
    expect(status(world({ h1: { own: 'crit' }, h2: { own: 'crit' } }), 'c')).toBe('crit');
  });

  it('shows a cluster whose source cannot be reached as degraded while a host still vouches for itself, and as down when none does', () => {
    expect(status(world({ c: { stale: true }, h1: { stale: false }, h2: { stale: true } }), 'c')).toBe('warn');
    expect(status(world({ c: { stale: true }, h1: { stale: true }, h2: { stale: true } }), 'c')).toBe('crit');
  });

  it('turns a volume yellow at the warning level and red at the critical one, from how full it is', () => {
    expect([withVolume(90).own, withVolume(90).reason]).toEqual(['warn', '90% used']);
    expect(withVolume(96).own).toBe('crit');
    expect([withVolume(10).own, withVolume(10).reason]).toEqual(['ok', '']);
  });

  it('keeps the identity of a node that did not change, so that nothing re-renders for it', () => {
    const s = world({ w1: { own: 'warn' } });
    const again = deriveStatuses(s.nodes, s.kids, s.settings, NOW + 1000);
    for (const id of s.nodes.keys()) expect(again.get(id)).toBe(s.nodes.get(id));
  });
});
