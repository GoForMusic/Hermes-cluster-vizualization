import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { SourceView } from '../generated/SourceView';
import { LogDock, type LogTarget } from '../features/topology/LogDock';
import { Topology } from '../features/admin/pages/Topology';
import { AdminShellContext } from '../features/admin/adminContext';
import { createHttpHubClient } from '../hub/HttpHubClient';
import type { IncidentAlert } from './incidentFixture';
import { renderWithHub } from './render';
import { IncidentReport } from '../features/admin/IncidentReport';

const target = (over: Partial<LogTarget> = {}): LogTarget => ({ source: 's1', namespace: 'shop', name: 'web-1', containers: ['app', 'sidecar'], ...over });

const open = (over: Partial<LogTarget> = {}, prepare?: (hub: import('./fakeHub').FakeHub) => void) =>
  renderWithHub(<LogDock target={target(over)} onClose={() => {}} />, { prepare });

describe('the log panel', () => {
  it('opens the log of the first container, follows it, and shows what comes', async () => {
    const { hub } = await open({}, (h) => { h.logLines = ['starting', 'ERROR: it broke', 'WARN: careful']; });
    expect(await screen.findByText('starting')).toBeInTheDocument();
    expect(hub.calls).toContain('logs.open:s1/shop/web-1:app:200:true:false:false:');
    expect(screen.getByText('ERROR: it broke')).toHaveClass('lg-err');
    expect(screen.getByText('WARN: careful')).toHaveClass('lg-warn');
    expect(screen.getByRole('status')).toHaveTextContent('Live');
    expect(screen.getByText('3 lines')).toBeInTheDocument();
  });

  it('starts again when what is read changes: another container, the one before the restart, the number of lines', async () => {
    const user = userEvent.setup();
    const { hub } = await open({}, (h) => { h.logLines = ['x']; });
    await screen.findByText('x');
    await user.selectOptions(screen.getByLabelText('Container'), 'sidecar');
    await waitFor(() => expect(hub.calls).toContain('logs.open:s1/shop/web-1:sidecar:200:true:false:false:'));
    await user.click(screen.getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(hub.calls).toContain('logs.open:s1/shop/web-1:sidecar:200:true:true:false:'));
    await user.selectOptions(screen.getByLabelText('Last lines'), '1000');
    await waitFor(() => expect(hub.calls).toContain('logs.open:s1/shop/web-1:sidecar:1000:true:true:false:'));
    await user.click(screen.getByRole('button', { name: 'Timestamps' }));
    await waitFor(() => expect(hub.calls).toContain('logs.open:s1/shop/web-1:sidecar:1000:true:true:true:'));
    // each start stopped the one before it: the agent is not left reading for nobody
    expect(hub.calls.filter((c) => c === 'logs.stop').length).toBe(4);
  });

  it('stops reading when it is closed', async () => {
    const user = userEvent.setup();
    let closed = false;
    const { hub, unmount } = await renderWithHub(<LogDock target={target()} onClose={() => { closed = true; }} />);
    await user.click(screen.getByRole('button', { name: 'Close the log' }));
    expect(closed).toBe(true);
    unmount();
    expect(hub.openLogs.every((l) => l.stopped)).toBe(true);
  });

  it('does not follow when asked not to, and says when the log ended', async () => {
    const user = userEvent.setup();
    const { hub } = await open({}, (h) => { h.logLines = ['one']; });
    await screen.findByText('one');
    await user.click(screen.getByRole('button', { name: 'Follow' }));
    await waitFor(() => expect(hub.calls).toContain('logs.open:s1/shop/web-1:app:200:false:false:false:'));
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Ended'));
  });

  it('shows why the log cannot be read', async () => {
    await open({}, (h) => { h.logError = 'this container has not been restarted: there is no earlier log'; });
    expect(await screen.findByRole('alert')).toHaveTextContent('there is no earlier log');
    expect(screen.getByRole('status')).toHaveTextContent('Failed');
  });

  it('searches the lines it has, marks the match and counts', async () => {
    const user = userEvent.setup();
    await open({}, (h) => { h.logLines = ['GET /health 200', 'POST /orders 500', 'GET /orders 200']; });
    await screen.findByText('POST /orders 500');
    await user.type(screen.getByLabelText('Search the log'), 'orders');
    expect(screen.getByText('2 of 3')).toBeInTheDocument();
    expect(screen.queryByText('GET /health 200')).not.toBeInTheDocument();
    expect(document.querySelectorAll('.logline mark')).toHaveLength(2);
  });

  it('shows no container choice for a workload that is one container', async () => {
    await open({ containers: [] }, (h) => { h.logLines = ['x']; });
    await screen.findByText('x');
    expect(screen.queryByLabelText('Container')).not.toBeInTheDocument();
  });

  it('keeps the last 20000 lines and says that the rest are not shown', async () => {
    const lines = Array.from({ length: 5200 }, (_, i) => `line ${i}`);
    await open({}, (h) => { h.logLines = lines; });
    expect(await screen.findByText('line 5199')).toBeInTheDocument();
    expect(screen.queryByText('line 0')).not.toBeInTheDocument();
    expect(screen.getByText(/200 earlier lines are not shown/)).toBeInTheDocument();
  });
});

describe('the log from the topology', () => {
  const source = (over: Partial<SourceView> = {}): SourceView => ({ id: 'c', name: 'lab', type: 'Kubernetes (agent)', endpoint: '', auth: '', state: 'connected', info: '', agents: [], canUpgrade: false, canLogs: true, ...over });
  const page = (over: Partial<SourceView> = {}, grants?: { path: string; capabilities: string[] }[]) =>
    renderWithHub(<AdminShellContext.Provider value={{ setCursor: () => {} }}><Topology arg="w" /></AdminShellContext.Provider>, {
      prepare: (hub) => { hub.sources = [source(over)]; hub.logLines = ['hello from the pod']; },
      session: { auth: { setupRequired: false, authenticated: true, publicView: false, username: 'admin', grants } },
    });

  it('opens the panel for the pod that is selected, at the bottom, and keeps the map', async () => {
    const user = userEvent.setup();
    const { hub, store } = await page();
    await act(async () => { await store.refreshSources(); });
    await user.click(await screen.findByRole('button', { name: 'Open the log' }));
    expect(await screen.findByText('hello from the pod')).toBeInTheDocument();
    expect(hub.calls.some((c) => c.startsWith('logs.open:c/default/'))).toBe(true);
    expect(screen.getByRole('heading', { name: 'Details' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Close the log' }));
    expect(screen.queryByLabelText(/Log of/)).not.toBeInTheDocument();
  });

  it('says how to turn reading logs on when the source was not installed to', async () => {
    const { store } = await page({ canLogs: false });
    await act(async () => { await store.refreshSources(); });
    expect(await screen.findByText(/Reading logs is off for this source/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open the log' })).not.toBeInTheDocument();
  });

  it('offers nothing to somebody whose policies do not let them read it', async () => {
    const { store } = await page({}, [{ path: 'sources/*', capabilities: ['read'] }]);
    await act(async () => { await store.refreshSources(); });
    await screen.findByRole('heading', { name: 'Details' });
    expect(screen.queryByRole('button', { name: 'Open the log' })).not.toBeInTheDocument();
    expect(screen.queryByText(/Reading logs is off/)).not.toBeInTheDocument();
  });

  it('offers it to somebody who may read that namespace only', async () => {
    const { store } = await page({}, [{ path: 'logs/c/default/*', capabilities: ['read'] }]);
    await act(async () => { await store.refreshSources(); });
    expect(await screen.findByRole('button', { name: 'Open the log' })).toBeInTheDocument();
  });
});

describe('the log through the real client', () => {
  /** A hub that answers with Server-Sent Events cut into pieces the way a network cuts them. */
  const hubSaying = (pieces: string[], status = 200, body?: unknown) => {
    const seen: string[] = [];
    const fetch = vi.fn(async (url: string, init?: RequestInit) => {
      seen.push(url);
      if (status !== 200) return new Response(JSON.stringify(body), { status });
      const enc = new TextEncoder();
      const stream = new ReadableStream({
        start(c) {
          pieces.forEach((p) => c.enqueue(enc.encode(p)));
          c.close();
        },
      });
      void init;
      return new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } });
    });
    return { fetch: fetch as unknown as typeof globalThis.fetch, seen };
  };
  const run = (hub: ReturnType<typeof hubSaying>, options: Partial<import('../hub/HubClient').LogOptions> = {}) =>
    new Promise<string[]>((resolve) => {
      const out: string[] = [];
      const client = createHttpHubClient({ fetch: hub.fetch });
      client.logs.open('s 1', 'shop', 'web-1', { tail: 50, follow: false, previous: true, timestamps: false, container: 'app', ...options }, {
        onLines: (l) => out.push(...l),
        onEnd: () => resolve([...out, '<end>']),
        onError: (m) => resolve([...out, `<error: ${m}>`]),
      });
    });

  it('reads lines and the end, whatever way the stream is cut, and asks with what it was given', async () => {
    const hub = hubSaying([': ping\n\ndata: {"lines":["a","b"]}\n', '\ndata: {"lin', 'es":["c"]}\n\nevent: end\ndata: {}\n\n']);
    expect(await run(hub)).toEqual(['a', 'b', 'c', '<end>']);
    expect(hub.seen[0]).toBe('/api/logs/s%201/shop/web-1?tail=50&previous=true&container=app');
  });

  it('turns a failure of the log into its reason', async () => {
    expect(await run(hubSaying(['data: {"lines":["x"]}\n\nevent: failed\ndata: {"error":"the pod is gone"}\n\n']))).toEqual(['x', '<error: the pod is gone>']);
  });

  it('tells what the hub refused with', async () => {
    expect(await run(hubSaying([], 409, { error: 'no agent of this source was installed to read logs' }))).toEqual(['<error: no agent of this source was installed to read logs>']);
  });

  it('says so when the connection closes without an end', async () => {
    expect(await run(hubSaying(['data: {"lines":["x"]}\n\n']))).toEqual(['x', '<error: The connection to the hub closed.>']);
  });

  it('stops quietly when it is stopped', async () => {
    let aborted = false;
    const fetch = vi.fn((_: string, init?: RequestInit) => new Promise<Response>((_, reject) => { init?.signal?.addEventListener('abort', () => { aborted = true; reject(new DOMException('x', 'AbortError')); }); }));
    const errors: string[] = [];
    const stop = createHttpHubClient({ fetch: fetch as unknown as typeof globalThis.fetch }).logs.open('s', 'n', 'p', { tail: 1, follow: true, previous: false, timestamps: false }, { onLines: () => {}, onEnd: () => {}, onError: (m) => errors.push(m) });
    stop();
    await waitFor(() => expect(aborted).toBe(true));
    expect(errors).toEqual([]);
  });
});

describe('the log in the incident report', () => {
  const alert = (logs?: string): IncidentAlert => ({ id: 1, key: 'k', sev: 'crit', nodeId: 'w1', title: 'Pod crashing', detail: '', ts: 1_700_000_000_000, resolvedTs: null, ack: false, snapshot: '', ...(logs ? { logs } : {}) });

  it('shows the last lines that were kept when the alert opened', async () => {
    await renderWithHub(<IncidentReport alert={alert('── before the last restart ──\npanic: cannot connect to db')} onClose={() => {}} onOpen={() => {}} />);
    expect(await screen.findByText(/panic: cannot connect to db/)).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'The log of the container' })).toBeInTheDocument();
  });

  it('shows nothing about a log when none was kept', async () => {
    await renderWithHub(<IncidentReport alert={alert()} onClose={() => {}} onOpen={() => {}} />);
    await screen.findByText('Pod crashing');
    expect(screen.queryByText(/The log of the container/)).not.toBeInTheDocument();
  });
});

void within;
