// IHubClient over HTTP (JSON) and Server-Sent Events.
import type { HubEvent } from '../generated/HubEvent';
import type { IEventFeed, FeedHandlers, IHubClient } from './HubClient';
import { HubError } from './HubClient';

type Fetch = typeof fetch;
type EventSourceFactory = (url: string) => EventSource;

export interface HttpOptions {
  fetch?: Fetch;
  eventSource?: EventSourceFactory;
  /** Called when a request is refused because there is no session: the app goes back to the login screen. */
  onAuthRequired?: () => void;
}

/** The header is the CSRF guard: a page from another origin cannot send it without a CORS preflight. */
const CSRF = { 'X-Requested-With': 'hermes' };

export function createHttpHubClient(options: HttpOptions = {}): IHubClient {
  const doFetch: Fetch = options.fetch ?? ((input, init) => fetch(input, init));
  const onAuth = options.onAuthRequired ?? (() => {});

  async function call<T>(method: string, url: string, body?: unknown): Promise<T> {
    const res = await doFetch(url, {
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
    },
    topology: {
      snapshot: () => get('/api/snapshot'),
      info: () => get('/api/info'),
    },
    alerts: {
      list: (limit = 300) => get(`/api/alerts?limit=${limit}`),
      ack: (id) => send('POST', `/api/alerts/${id}/ack`),
    },
    settings: {
      load: () => get('/api/settings'),
      save: (settings) => send('PUT', '/api/settings', settings),
    },
    sources: {
      list: () => get('/api/sources'),
      add: (request) => call('POST', '/api/sources', request),
      rename: (id, name) => send('PATCH', `/api/sources/${encodeURIComponent(id)}`, { name }),
      remove: (id) => send('DELETE', `/api/sources/${encodeURIComponent(id)}`),
      upgrade: (id, version) => send('POST', `/api/sources/${encodeURIComponent(id)}/upgrade`, { version }),
    },
    registry: {
      get: () => get('/api/registry'),
      save: (input) => call('PUT', '/api/registry', input),
      test: (input) => call('POST', '/api/registry/test', input),
      versions: () => get('/api/registry/versions'),
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
    uptime: {
      load: (span, buckets) => get(`/api/uptime?span=${span}&buckets=${buckets}`),
    },
    feed: sseFeed(options.eventSource ?? ((url) => new EventSource(url))),
  };
}

/** `EventSource` reconnects on its own and every connection starts with a snapshot, so a dropped link heals itself. */
function sseFeed(create: EventSourceFactory): IEventFeed {
  return {
    connect(handlers: FeedHandlers) {
      const es = create('/api/stream');
      es.onmessage = (e: MessageEvent<string>) => handlers.onEvent(JSON.parse(e.data) as HubEvent);
      es.onopen = () => handlers.onOpen();
      es.onerror = () => handlers.onError(es.readyState === EventSource.CLOSED);
      return () => es.close();
    },
  };
}
