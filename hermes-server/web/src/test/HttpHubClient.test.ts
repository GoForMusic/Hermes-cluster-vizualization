import { describe, expect, it, vi } from 'vitest';
import { HubError } from '../hub/HubClient';
import { createHttpHubClient } from '../hub/HttpHubClient';

const answer = (body: unknown, status = 200): Response => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });

describe('the HTTP client', () => {
  it('sends the CSRF header on every call, and a body only when there is one', async () => {
    const fetch = vi.fn(async () => answer({ ok: true }));
    const client = createHttpHubClient({ fetch });
    await client.auth.login({ username: 'admin', password: 'x' });
    await client.topology.info();
    const [login, info] = fetch.mock.calls as unknown as [string, RequestInit][][];
    expect(login).toEqual(['/api/auth/login', expect.objectContaining({ method: 'POST', body: '{"username":"admin","password":"x"}', headers: { 'X-Requested-With': 'hermes', 'content-type': 'application/json' } })]);
    expect(info).toEqual(['/api/info', expect.objectContaining({ method: 'GET', headers: { 'X-Requested-With': 'hermes' } })]);
  });

  it('reads the calls the screens make', async () => {
    const urls: string[] = [];
    const fetch = vi.fn(async (url: string) => { urls.push(url); return answer({}); });
    const c = createHttpHubClient({ fetch: fetch as never });
    await c.alerts.list(); await c.alerts.ack(7); await c.uptime.load(3600, 60); await c.sources.remove('a b'); await c.settings.save({} as never);
    expect(urls).toEqual(['/api/alerts?limit=300', '/api/alerts/7/ack', '/api/uptime?span=3600&buckets=60', '/api/sources/a%20b', '/api/settings']);
  });

  it('turns an error answer into a HubError with the hub\'s own words', async () => {
    const client = createHttpHubClient({ fetch: async () => answer({ error: 'wrong username or password' }, 401) });
    const error = await client.auth.login({ username: 'a', password: 'b' }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(HubError);
    expect(error).toMatchObject({ message: 'wrong username or password', status: 401, authRequired: false });
  });

  it('says the login is needed only when the hub says so, and tells the app', async () => {
    const onAuthRequired = vi.fn();
    const client = createHttpHubClient({ fetch: async () => answer({ error: 'login required', auth: true }, 401), onAuthRequired });
    await expect(client.sources.list()).rejects.toMatchObject({ authRequired: true, status: 401 });
    expect(onAuthRequired).toHaveBeenCalledOnce();
  });

  it('survives an answer that is not JSON', async () => {
    const client = createHttpHubClient({ fetch: async () => new Response('bad gateway', { status: 502, statusText: 'Bad Gateway' }) });
    await expect(client.topology.info()).rejects.toMatchObject({ message: 'Bad Gateway', status: 502 });
  });

  it('follows the event stream: events, opening, errors, and closing', () => {
    class FakeSource {
      static CLOSED = 2;
      readyState = 1;
      closed = false;
      onmessage: ((e: { data: string }) => void) | null = null;
      onopen: (() => void) | null = null;
      onerror: (() => void) | null = null;
      close() { this.closed = true; }
    }
    const es = new FakeSource();
    vi.stubGlobal('EventSource', FakeSource);
    const client = createHttpHubClient({ eventSource: () => es as never });
    const seen: string[] = [];
    const stop = client.feed.connect({ onEvent: (e) => seen.push(e.type), onOpen: () => seen.push('open'), onError: (refused) => seen.push(`error:${refused}`) });
    es.onopen!();
    es.onmessage!({ data: '{"type":"sources"}' });
    es.onerror!();
    es.readyState = FakeSource.CLOSED;
    es.onerror!();
    stop();
    expect(seen).toEqual(['open', 'sources', 'error:false', 'error:true']);
    expect(es.closed).toBe(true);
    vi.unstubAllGlobals();
  });
});
