// A hub that lives in memory: what a test hands to the store and the screens instead of `fetch`.
import type { HubEvent } from '../generated/HubEvent';
import type { IHubClient, FeedHandlers } from '../hub/HubClient';
import type { Alert, Uptime } from '../domain/model';
import type { Settings } from '../domain/settings';
import type { AuthStatus } from '../generated/AuthStatus';
import type { CommLogEntry } from '../generated/CommLogEntry';
import type { CommLogView } from '../generated/CommLogView';
import type { RegistryTest } from '../generated/RegistryTest';
import type { RegistryView } from '../generated/RegistryView';
import type { SourceView } from '../generated/SourceView';
import { wireEdge, wireNode } from './fixtures';

export interface FakeHub {
  client: IHubClient;
  /** what the fake was asked to do, in order */
  calls: string[];
  saved: Settings[];
  /** the live feed: what the store connected with, so a test can push events through it */
  feed: { handlers: FeedHandlers | null; closed: boolean };
  uptime: Record<string, Uptime>;
  sources: SourceView[];
  registry: RegistryView;
  /** when set, asking for a version change fails with this */
  upgradeError: string;
  /** When set, renaming a source fails with this message (a name already taken). */
  renameError: string;
  /** the communication log the fake hub holds (`entries` are the rows, `bodies` their content by id) */
  commLog: CommLogView;
  bodies: Record<number, unknown>;
  /** the incident history the fake hub holds */
  alerts: Alert[];
  /** what "Test connection" and the version list answer */
  registryTest: RegistryTest;
  data: { nodes: ReturnType<typeof wireNode>[]; edges: ReturnType<typeof wireEdge>[] };
}

export function fakeHub(auth: Partial<AuthStatus> = {}): FakeHub {
  const hub: FakeHub = {
    calls: [],
    saved: [],
    feed: { handlers: null, closed: false },
    uptime: {},
    sources: [],
    upgradeError: '',
    commLog: { enabled: false, capacity: 500, minutes: 15, heartbeats: 0, entries: [], newest: 0 },
    bodies: {},
    alerts: [],
    registry: { url: '', project: '', auth: 'none', username: '', hasSecret: false, linuxImage: 'hermes-agent-linux', windowsImage: 'hermes-agent-windows', implicit: false },
    renameError: '',
    registryTest: { ok: true, message: 'connected', versions: ['1.0.1', '1.0.0'] },
    data: { nodes: [wireNode('c', 'cluster', null), wireNode('h', 'host', 'c'), wireNode('w', 'workload', 'h', { meta: { ns: 'default' } })], edges: [] },
    client: null as unknown as IHubClient,
  };
  const note = (call: string): void => { hub.calls.push(call); };
  hub.client = {
    auth: {
      status: async () => ({ setupRequired: false, authenticated: true, publicView: false, username: 'admin', ...auth }),
      setup: async (c) => note(`auth.setup:${c.username}:${c.publicView}`),
      login: async (c) => { note(`auth.login:${c.username}`); if (c.password === 'wrong') throw new Error('wrong username or password'); },
      logout: async () => note('auth.logout'),
      changePassword: async (current, next) => note(`auth.changePassword:${current}:${next}`),
      setPublicView: async (on) => note(`auth.publicView:${on}`),
    },
    topology: {
      snapshot: async () => { note('snapshot'); return hub.data; },
      info: async () => ({ demo: false, sourceTypes: ['Docker Swarm (agent)', 'Kubernetes (agent)'], version: '1.0.0' }),
    },
    alerts: { list: async () => { note('alerts.list'); return hub.alerts; }, ack: async (id) => note(`alerts.ack:${id}`) },
    settings: { load: async () => ({}), save: async (s) => { hub.saved.push(s); } },
    sources: {
      list: async () => { note('sources.list'); return hub.sources; },
      add: async (r) => { note(`sources.add:${r.name}`); note(`sources.flows:${r.flows === true}`); note(`sources.version:${r.version ?? ''}`); note(`sources.upgrades:${r.upgrades === true}`); return { id: 's1', name: r.name, type: r.type, endpoint: '', auth: '', state: 'pending', info: '', install: 'yaml', hint: 'apply it' }; },
      rename: async (id, name) => { note(`sources.rename:${id}:${name}`); if (hub.renameError) throw new Error(hub.renameError); },
      remove: async (id) => note(`sources.remove:${id}`),
      upgrade: async (id, version) => { note(`sources.upgrade:${id}:${version}`); if (hub.upgradeError) throw new Error(hub.upgradeError); },
    },
    registry: {
      get: async () => hub.registry,
      save: async (i) => { note(`registry.save:${i.url}:${i.auth}:${i.secret ?? '-'}`); hub.registry = { url: i.url, project: i.project, auth: i.auth, username: i.username, hasSecret: i.secret ? true : hub.registry.hasSecret, linuxImage: i.linuxImage, windowsImage: i.windowsImage, implicit: false }; return hub.registry; },
      test: async (i) => { note(`registry.test:${i.url}`); return hub.registryTest; },
      versions: async () => hub.registryTest,
    },
    commLog: {
      list: async (q) => {
        note(`commLog.list:${q?.source ?? ''}:${q?.kind ?? ''}:${q?.q ?? ''}`);
        return hub.commLog;
      },
      get: async (id) => {
        const row = hub.commLog.entries.find((e) => e.id === id);
        if (!row) throw new Error('that entry is gone');
        return { ...row, body: hub.bodies[id] ?? null } as CommLogEntry;
      },
      enable: async (on) => { note(`commLog.enable:${on}`); hub.commLog = { ...hub.commLog, enabled: on, entries: on ? hub.commLog.entries : [] }; },
      clear: async () => { note('commLog.clear'); hub.commLog = { ...hub.commLog, entries: [] }; },
    },
    uptime: { load: async (span) => { note(`uptime:${span}`); return hub.uptime; } },
    feed: { connect: (handlers) => { hub.feed.handlers = handlers; hub.feed.closed = false; return () => { hub.feed.closed = true; }; } },
  };
  return hub;
}

export const push = (hub: FakeHub, event: HubEvent): void => hub.feed.handlers!.onEvent(event);
