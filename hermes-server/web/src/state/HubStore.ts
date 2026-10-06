// The app's state and what changes it. It holds one immutable `HubState`; every event and every action makes a new one (through the pure
// reducer) and tells the subscribers. It talks to the hub only through `IHubClient`, so it runs against a fake in the tests.
import type { AddSourceRequest } from '../generated/AddSourceRequest';
import type { AddSourceResponse } from '../generated/AddSourceResponse';
import type { HubEvent } from '../generated/HubEvent';
import { RANGES, initialState, type HubState, type UptimeRange } from '../domain/hubState';
import type { Alert } from '../domain/model';
import { boot, reduce } from '../domain/reducer';
import { mergeSettings, type Settings } from '../domain/settings';
import type { IHubClient } from '../hub/HubClient';

type Listener = () => void;

export class HubStore {
  private state: HubState = initialState();
  private readonly listeners = new Set<Listener>();
  private readonly alertListeners = new Set<(alert: Alert) => void>();

  constructor(
    private readonly client: IHubClient,
    private readonly clock: () => number = Date.now,
  ) {}

  // ---- what React reads (useSyncExternalStore) ----------------------------------------------------------------

  getState = (): HubState => this.state;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  /** Told when an alert opens (not when one changes): the wallboard puts it on the kill feed and may beep. */
  onNewAlert(listener: (alert: Alert) => void): () => void {
    this.alertListeners.add(listener);
    return () => this.alertListeners.delete(listener);
  }

  private set(next: HubState): void {
    if (next === this.state) return;
    this.state = next;
    this.listeners.forEach((l) => l());
  }

  // ---- loading and the live stream ------------------------------------------------------------------------------

  /** Loads what the person may see. Source details are for admins only. */
  async load(admin: boolean): Promise<void> {
    const { topology, settings, alerts, sources } = this.client;
    const [snapshot, info, saved, list] = await Promise.all([topology.snapshot(), topology.info(), settings.load(), alerts.list()]);
    const sourceList = admin ? await sources.list() : [];
    this.set(boot(this.state, { nodes: snapshot.nodes, edges: snapshot.edges, info, settings: saved, alerts: list, sources: sourceList }, this.clock()));
  }

  /** Follows the hub until the returned function is called. `onRefused`: the hub does not let this browser listen any more. */
  startLive(onRefused: () => void = () => {}): () => void {
    let lost = false;
    let first = true;
    const stopFeed = this.client.feed.connect({
      onEvent: (event) => {
        if (event.type === 'snapshot' && first) { first = false; return; } // the same data that was loaded over REST
        this.apply(event);
      },
      onOpen: () => {
        this.set({ ...this.state, link: 'live' });
        if (lost) { // catch up on what was missed while the link was down
          lost = false;
          void this.refreshAlerts();
          void this.refreshSettings();
          void this.refreshUptime();
          void this.refreshInfo();
        }
      },
      onError: (refused) => {
        lost = true;
        this.set({ ...this.state, link: 'lost' });
        if (refused) onRefused();
      },
    });
    void this.refreshUptime();
    const timer = setInterval(() => void this.refreshUptime(), 30_000);
    return () => { stopFeed(); clearInterval(timer); };
  }

  apply(event: HubEvent): void {
    const before = this.state.range;
    const r = reduce(this.state, event, this.clock());
    this.set(r.state);
    if (r.newAlert) this.alertListeners.forEach((l) => l(r.newAlert!));
    if (r.refreshSources) void this.refreshSources();
    // A settings change can move an unpinned session's range (see `withDefaultRange`): fetch that range's bars now, rather than
    // waiting for the next 30s tick, so a shared default-range change is visibly live.
    if (r.state.range.id !== before.id) void this.refreshUptime();
  }

  // ---- fetching again -------------------------------------------------------------------------------------------

  async refreshAlerts(): Promise<void> {
    this.set({ ...this.state, alerts: await this.client.alerts.list() });
  }

  async refreshSettings(): Promise<void> {
    const settings = mergeSettings(await this.client.settings.load());
    this.apply({ type: 'settings', settings: settings as unknown as Record<string, unknown> });
  }

  /** After the link came back: the hub may have been updated meanwhile, which the page should say. */
  async refreshInfo(): Promise<void> {
    try { this.set({ ...this.state, info: await this.client.topology.info() }); } catch { /* still unreachable: the link tag says so */ }
  }

  async refreshSources(): Promise<void> {
    try { this.set({ ...this.state, sources: await this.client.sources.list() }); } catch { /* the hub is unreachable: keep the old list */ }
  }

  async refreshUptime(): Promise<void> {
    try {
      const range = this.state.range;
      const data = await this.client.uptime.load(range.span, range.buckets);
      if (this.state.rangeAuto) {
        const firsts = Object.values(data).map((u) => u.first).filter(Boolean);
        if (firsts.length) {
          const age = (this.clock() - Math.min(...firsts)) / 1000;
          const pick = RANGES.find((r) => r.span >= age) ?? RANGES[RANGES.length - 1]!;
          if (pick.id !== range.id) {
            this.set({ ...this.state, range: pick });
            return this.refreshUptime();
          }
        }
      }
      this.set({ ...this.state, uptime: new Map(Object.entries(data)) });
    } catch { /* try again on the next tick */ }
  }

  // ---- what people do -------------------------------------------------------------------------------------------

  /** Applies at once and saves on the hub, so every open screen (and TV) follows. */
  updateSettings(change: (current: Settings) => Settings): void {
    const settings = change(this.state.settings);
    this.apply({ type: 'settings', settings: settings as unknown as Record<string, unknown> });
    this.client.settings.save(settings).catch((e: unknown) => console.error('saving settings failed:', e));
  }

  async ackAlert(id: number): Promise<void> {
    await this.client.alerts.ack(id).catch((e: unknown) => console.error(e));
  }

  setRange(range: UptimeRange): void {
    this.set({ ...this.state, range, rangeAuto: false, rangePinned: true });
    void this.refreshUptime();
  }

  async addSource(request: AddSourceRequest): Promise<AddSourceResponse> {
    const added = await this.client.sources.add(request);
    void this.refreshSources();
    return added;
  }

  async upgradeSource(id: string, version: string): Promise<void> {
    await this.client.sources.upgrade(id, version);
    await this.refreshSources();
  }

  async renameSource(id: string, name: string): Promise<void> {
    await this.client.sources.rename(id, name);
    await this.refreshSources();
  }

  async removeSource(id: string): Promise<void> {
    await this.client.sources.remove(id);
    await this.refreshSources();
  }
}
