// IHubClient over HTTP (JSON) and Server-Sent Events.
import type { HubEvent } from '../generated/HubEvent';
import type { IEventFeed, FeedHandlers, IHubClient } from './HubClient';
import { withBase } from './base';
import { HubError } from './HubClient';

type Fetch = typeof fetch;
type EventSourceFactory = (url: string) => EventSource;

export interface HttpOptions {
  /** The page is the wallboard (TV): what it reads is asked for as the wallboard's, which shows every cluster when the admin made it public. */
  wallboard?: () => boolean;
  fetch?: Fetch;
  eventSource?: EventSourceFactory;
  /** Called when a request is refused because there is no session: the app goes back to the login screen. */
  onAuthRequired?: () => void;
}

/** The admin API of this hub, or of one of its datacenters (the hub carries the call there). */
const root = (dc?: string): string => (dc ? `/api/dc/${encodeURIComponent(dc)}` : '/api');

/** The header is the CSRF guard: a page from another origin cannot send it without a CORS preflight. */
const CSRF = { 'X-Requested-With': 'hermes' };

export function createHttpHubClient(options: HttpOptions = {}): IHubClient {
  /** The reads that the wallboard makes carry `view=tv`: the hub then shows the wallboard's clusters, not only the ones the person's policies give. */
  const tv = (path: string): string => (options.wallboard?.() ? `${path}${path.includes('?') ? '&' : '?'}view=tv` : path);
  const doFetch: Fetch = options.fetch ?? ((input, init) => fetch(input, init));
  const onAuth = options.onAuthRequired ?? (() => {});

  async function call<T>(method: string, url: string, body?: unknown): Promise<T> {
    const res = await doFetch(withBase(url), {
      method,
      cache: 'no-store',
      headers: { ...CSRF, ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const data = (await res.json().catch(() => null)) as { error?: string; auth?: boolean } | null;
    if (!res.ok) {
      const authRequired = res.status === 401 && data?.auth === true;
      if (authRequired) onAuth();
      throw new HubError(data?.error ?? res.statusText, res.status, authRequired);
    }
    return data as T;
  }
  const get = <T>(url: string): Promise<T> => call<T>('GET', url);
  const send = async (method: string, url: string, body?: unknown): Promise<void> => {
    await call(method, url, body ?? {});
  };

  return {
    auth: {
      status: () => get('/api/auth/status'),
      setup: (c) => send('POST', '/api/auth/setup', c),
      login: (c) => send('POST', '/api/auth/login', c),
      logout: () => send('POST', '/api/auth/logout'),
      changePassword: (current, next) => send('POST', '/api/auth/password', { current, next }),
      setPublicView: (enabled) => send('PUT', '/api/auth/public-view', { enabled }),
      sessionPolicy: () => get('/api/security'),
      setSessionPolicy: (policy) => call('PUT', '/api/security', policy),
    },
    topology: {
      snapshot: () => get(tv('/api/snapshot')),
      info: () => get(tv('/api/info')),
    },
    alerts: {
      list: (limit = 300) => get(tv(`/api/alerts?limit=${limit}`)),
      ack: (id) => send('POST', `/api/alerts/${id}/ack`),
    },
    settings: {
      load: () => get(tv('/api/settings')),
      save: (settings) => send('PUT', '/api/settings', settings),
    },
    sources: {
      list: () => get('/api/sources'),
      add: (request, dc) => call('POST', `${root(dc)}/sources`, request),
      rename: (id, name, dc) => send('PATCH', `${root(dc)}/sources/${encodeURIComponent(id)}`, { name }),
      remove: (id, dc) => send('DELETE', `${root(dc)}/sources/${encodeURIComponent(id)}`),
      upgrade: (id, version, dc) => send('POST', `${root(dc)}/sources/${encodeURIComponent(id)}/upgrade`, { version }),
    },
    registry: {
      get: (dc) => get(`${root(dc)}/registry`),
      save: (input, dc) => call('PUT', `${root(dc)}/registry`, input),
      test: (input, dc) => call('POST', `${root(dc)}/registry/test`, input),
      versions: (dc) => get(`${root(dc)}/registry/versions`),
    },
    federation: {
      view: () => get('/api/federation'),
      enableMember: (agentAddress) => call('POST', '/api/federation/member', { agentAddress }),
      newCode: () => call('POST', '/api/federation/member/code', {}),
      disableMember: () => send('DELETE', '/api/federation/member'),
      addDatacenter: (input) => call('POST', '/api/federation/datacenters', input),
      updateDatacenter: (id, patch) => call('PATCH', `/api/federation/datacenters/${encodeURIComponent(id)}`, patch),
      removeDatacenter: (id) => call('DELETE', `/api/federation/datacenters/${encodeURIComponent(id)}`, {}),
    },
    commLog: {
      list: (query = {}) => {
        const p = new URLSearchParams();
        for (const [k, v] of Object.entries(query)) if (v !== undefined && v !== '') p.set(k, String(v));
        return get(`/api/comm-log${p.size ? `?${p}` : ''}`);
      },
      get: (id) => get(`/api/comm-log/${id}`),
      enable: (enabled) => send('PUT', '/api/comm-log', { enabled }),
      clear: () => send('DELETE', '/api/comm-log'),
    },
    access: {
      users: () => get('/api/users'),
      createUser: (input) => call('POST', '/api/users', input),
      updateUser: (id, patch) => call('PATCH', `/api/users/${id}`, patch),
      deleteUser: (id) => send('DELETE', `/api/users/${id}`),
      groups: () => get('/api/groups'),
      createGroup: (input) => call('POST', '/api/groups', input),
      updateGroup: (id, patch) => call('PATCH', `/api/groups/${id}`, patch),
      deleteGroup: (id) => send('DELETE', `/api/groups/${id}`),
      setMembers: (id, users) => call('PUT', `/api/groups/${id}/members`, { users }),
      setGroupPolicies: (id, policies) => call('PUT', `/api/groups/${id}/policies`, { policies }),
      policies: () => get('/api/policies'),
      createPolicy: (input) => call('POST', '/api/policies', input),
      updatePolicy: (id, patch) => call('PATCH', `/api/policies/${id}`, patch),
      deletePolicy: (id) => send('DELETE', `/api/policies/${id}`),
      check: (user, capability, path) => get(`/api/policies/check?user=${user}&capability=${encodeURIComponent(capability)}&path=${encodeURIComponent(path)}`),
    },
    sso: {
      providers: () => get('/api/identity-providers'),
      create: (input) => call('POST', '/api/identity-providers', input),
      update: (id, input) => call('PUT', `/api/identity-providers/${encodeURIComponent(id)}`, input),
      remove: (id) => send('DELETE', `/api/identity-providers/${encodeURIComponent(id)}`),
      test: (issuer) => call('POST', '/api/identity-providers/test', { issuer }),
    },
    logs: {
      open(source, namespace, name, options, handlers) {
        const stop = new AbortController();
        const q = new URLSearchParams({ tail: String(options.tail) });
        if (options.follow) q.set('follow', 'true');
        if (options.previous) q.set('previous', 'true');
        if (options.timestamps) q.set('timestamps', 'true');
        if (options.container) q.set('container', options.container);
        if (options.task) q.set('task', options.task);
        const path = [source, namespace, name].map(encodeURIComponent).join('/');
        void (async () => {
          let finished = false;
          try {
            const res = await doFetch(withBase(`/api/logs/${path}?${q}`), { signal: stop.signal, cache: 'no-store' });
            if (!res.ok) {
              const data = (await res.json().catch(() => null)) as { error?: string; auth?: boolean } | null;
              if (res.status === 401 && data?.auth === true) onAuth();
              finished = true;
              handlers.onError(data?.error ?? res.statusText);
              return;
            }
            await readEvents(res, (event, data) => {
              if (event === 'end') { finished = true; handlers.onEnd(); }
              else if (event === 'failed') { finished = true; handlers.onError((JSON.parse(data) as { error: string }).error); }
              else if (event === 'message') handlers.onLines((JSON.parse(data) as { lines: string[] }).lines);
            });
            if (!finished && !stop.signal.aborted) handlers.onError('The connection to the hub closed.');
          } catch (e) {
            if (!stop.signal.aborted && !finished) handlers.onError(e instanceof Error ? e.message : String(e));
          }
        })();
        return () => stop.abort();
      },
    },
    uptime: {
      load: (span, buckets) => get(tv(`/api/uptime?span=${span}&buckets=${buckets}`)),
    },
    feed: sseFeed(options.eventSource ?? ((url) => new EventSource(url)), () => tv('/api/stream')),
  };
}

/** `EventSource` reconnects on its own and every connection starts with a snapshot, so a dropped link heals itself. */
function sseFeed(create: EventSourceFactory, path: () => string): IEventFeed {
  return {
    connect(handlers: FeedHandlers) {
      const es = create(withBase(path()));
      es.onmessage = (e: MessageEvent<string>) => handlers.onEvent(JSON.parse(e.data) as HubEvent);
      es.onopen = () => handlers.onOpen();
      es.onerror = () => handlers.onError(es.readyState === EventSource.CLOSED);
      return () => es.close();
    },
  };
}

/** Reads a Server-Sent Events response as it arrives: `onEvent(name, data)` for each event ("message" when it has no name). Comments are skipped. */
async function readEvents(res: Response, onEvent: (event: string, data: string) => void): Promise<void> {
  const reader = res.body?.getReader();
  if (!reader) return;
  const decoder = new TextDecoder();
  let buffer = '';
  const dispatch = (block: string): void => {
    let event = 'message';
    const data: string[] = [];
    for (const line of block.split('\n')) {
      if (line.startsWith('event:')) event = line.slice(6).trim();
      else if (line.startsWith('data:')) data.push(line.slice(5).replace(/^ /, ''));
    }
    if (data.length) onEvent(event, data.join('\n'));
  };
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true }).replace(/\r\n/g, '\n');
    let at: number;
    while ((at = buffer.indexOf('\n\n')) >= 0) {
      dispatch(buffer.slice(0, at));
      buffer = buffer.slice(at + 2);
    }
  }
}
