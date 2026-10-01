import { describe, expect, it, vi } from 'vitest';
import { RANGES } from '../domain/hubState';
import { fakeHub, push } from './fakeHub';
import { HubStore } from '../state/HubStore';

const NOW = 1_700_000_000_000;

async function loaded(admin = true) {
  const hub = fakeHub();
  const store = new HubStore(hub.client, () => NOW);
  await store.load(admin);
  return { hub, store };
}

describe('the store', () => {
  it('loads what the person may see: source details only for an admin', async () => {
    const admin = await loaded(true);
    expect(admin.hub.calls).toContain('sources.list');
    const viewer = await loaded(false);
    expect(viewer.hub.calls).not.toContain('sources.list');
    const s = viewer.store.getState();
    expect([s.nodes.size, s.info.version, s.bootVersion]).toEqual([3, '1.0.0', '1.0.0']);
  });

  it('applies the live events and tells its subscribers, once per change', async () => {
    const { hub, store } = await loaded();
    const listener = vi.fn();
    store.subscribe(listener);
    store.startLive();
    push(hub, { type: 'snapshot', nodes: [], edges: [] }); // the first snapshot repeats what was loaded: ignored
    expect(listener).not.toHaveBeenCalled();
    push(hub, { type: 'status', id: 'h', own: 'crit', reason: 'down' });
    expect(store.getState().nodes.get('h')!.status).toBe('crit');
    expect(listener).toHaveBeenCalledTimes(1);
    push(hub, { type: 'snapshot', nodes: [], edges: [] }); // later snapshots are real
    expect(store.getState().nodes.size).toBe(0);
  });

  it('announces an alert that opens, and not one that changes', async () => {
    const { hub, store } = await loaded();
    const heard: number[] = [];
    store.onNewAlert((a) => heard.push(a.id));
    store.startLive();
    const alert = { id: 1, key: 'k', sev: 'crit' as const, nodeId: 'w', title: 't', detail: '', ts: 1, resolvedTs: null, ack: false, snapshot: '' };
    push(hub, { type: 'alert', alert, isNew: true });
    push(hub, { type: 'alert', alert: { ...alert, title: 'changed' }, isNew: false });
    expect(heard).toEqual([1]);
    expect(store.getState().alerts[0]!.title).toBe('changed');
  });

  it('fetches the sources again when the hub says they changed', async () => {
    const { hub, store } = await loaded();
    store.startLive();
    hub.calls.length = 0;
    hub.sources = [{ id: 's', name: 'lab', type: 'Docker Swarm (agent)', endpoint: '', auth: '', state: 'connected', info: '', agents: [], canUpgrade: false }];
    push(hub, { type: 'sources' });
    await vi.waitFor(() => expect(store.getState().sources).toHaveLength(1));
    expect(hub.calls).toContain('sources.list');
  });

  it('says the link is lost and catches up when it comes back', async () => {
    const { hub, store } = await loaded();
    store.startLive();
    hub.feed.handlers!.onError(false);
    expect(store.getState().link).toBe('lost');
    hub.calls.length = 0;
    hub.feed.handlers!.onOpen();
    expect(store.getState().link).toBe('live');
    await vi.waitFor(() => expect(hub.calls).toEqual(expect.arrayContaining(['alerts.list'])));
  });

  it('tells the app when the hub refuses the stream', async () => {
    const { hub, store } = await loaded();
    const refused = vi.fn();
    store.startLive(refused);
    hub.feed.handlers!.onError(false);
    expect(refused).not.toHaveBeenCalled();
    hub.feed.handlers!.onError(true);
    expect(refused).toHaveBeenCalledOnce();
  });

  it('stops following the hub when asked', async () => {
    const { hub, store } = await loaded();
    store.startLive()();
    expect(hub.feed.closed).toBe(true);
  });

  it('applies a settings change at once and saves it on the hub', async () => {
    const { hub, store } = await loaded();
    store.updateSettings((s) => ({ ...s, sidebar: false }));
    expect(store.getState().settings.sidebar).toBe(false);
    expect(hub.saved.map((s) => s.sidebar)).toEqual([false]);
  });

  it('picks the smallest uptime range that shows all the history, until a person picks one', async () => {
    const { hub, store } = await loaded();
    hub.uptime = { h: { bars: [], pct: 100, current: 'up', first: NOW - 5 * 3600 * 1000 } }; // five hours old
    await store.refreshUptime();
    expect(store.getState().range.id).toBe('6h');
    expect(store.getState().uptime.get('h')!.pct).toBe(100);
    store.setRange(RANGES[0]!);
    await vi.waitFor(() => expect(hub.calls).toContain('uptime:3600'));
    expect(store.getState().range.id).toBe('1h');
    expect(store.getState().rangeAuto).toBe(false);
  });

  it('adds and removes sources through the hub', async () => {
    const { hub, store } = await loaded();
    const added = await store.addSource({ name: 'lab', type: 'Docker Swarm (agent)', endpoint: '', hubUrl: 'http://hub' });
    expect(added.install).toBe('yaml');
    await store.removeSource('s1');
    expect(hub.calls).toEqual(expect.arrayContaining(['sources.add:lab', 'sources.remove:s1']));
  });
});
