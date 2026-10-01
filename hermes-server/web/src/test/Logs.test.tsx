import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { CommLogRow } from '../generated/CommLogRow';
import { Logs } from '../features/admin/pages/Logs';
import { renderWithHub } from './render';

const row = (id: number, over: Partial<CommLogRow> = {}): CommLogRow => ({ id, ts: 1_700_000_000_000 + id * 1000, source: 's1', sourceName: 'lab-k8s', agent: 'infraviz-agent-x', dir: 'in', kind: 'batch', summary: 'snapshot ×1 · metrics ×1', bytes: 2048, ...over });

async function open(over: { enabled?: boolean; entries?: CommLogRow[]; heartbeats?: number; bodies?: Record<number, unknown> } = {}) {
  const rig = await renderWithHub(<Logs />, {
    prepare: (h) => { h.commLog = { enabled: over.enabled ?? true, capacity: 500, minutes: 15, heartbeats: over.heartbeats ?? 0, entries: over.entries ?? [], newest: 0 }; h.bodies = over.bodies ?? {}; },
  });
  return { ...rig, user: userEvent.setup() };
}

describe('the communication log page', () => {
  it('is off by default and says what turning it on means', async () => {
    const { hub, user } = await open({ enabled: false });
    expect(await screen.findByText('The log is off')).toBeInTheDocument();
    expect(screen.getByText(/never tokens, and only you see it/)).toBeInTheDocument();
    await user.click(screen.getByRole('checkbox', { name: 'Record the agents’ traffic' }));
    await waitFor(() => expect(hub.calls).toContain('commLog.enable:true'));
    expect(await screen.findByText('Nothing yet')).toBeInTheDocument();
  });

  it('lists what was said, newest first, with the direction, the kind and the size, and counts the heartbeats', async () => {
    await open({ entries: [row(3, { dir: 'out', kind: 'resync', summary: 'send the snapshot again', bytes: 0 }), row(2), row(1, { kind: 'connected', dir: 'link', summary: 'hello', bytes: 40 })], heartbeats: 1234 });
    expect(await screen.findByText('send the snapshot again')).toBeInTheDocument();
    expect(screen.getByText('snapshot ×1 · metrics ×1')).toBeInTheDocument();
    expect(screen.getByText('2.0 KiB')).toBeInTheDocument();
    expect(screen.getByText('← agent')).toBeInTheDocument();
    expect(screen.getByText(/1,234 heartbeats/)).toBeInTheDocument();
    expect(screen.getByText(/3 shown/)).toBeInTheDocument();
  });

  it('opens the content of a row as JSON, and copies it', async () => {
    const { user } = await open({ entries: [row(2)], bodies: { 2: { events: [{ kind: 'status', content: { id: 's1:w:checkout', own: 'crit', reason: 'ImagePullBackOff' } }] } } });
    await user.click(await screen.findByText('snapshot ×1 · metrics ×1'));
    expect(await screen.findByText(/"reason": "ImagePullBackOff"/)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Copy JSON' }));
    expect(await navigator.clipboard.readText()).toContain('ImagePullBackOff');
    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByText(/"reason"/)).not.toBeInTheDocument();
  });

  it('says so when the entry has already fallen out of the log', async () => {
    const { user, hub } = await open({ entries: [row(2)] });
    const shown = await screen.findByText('snapshot ×1 · metrics ×1');
    hub.commLog = { ...hub.commLog, entries: [] }; // it fell out between the list and the click
    await user.click(shown);
    expect(await screen.findByText(/That entry is gone/)).toBeInTheDocument();
  });

  it('asks the hub to filter by source, kind and words', async () => {
    const { hub, user } = await open({ entries: [row(1)] });
    await screen.findByText('snapshot ×1 · metrics ×1');
    await user.selectOptions(screen.getByLabelText('Kind'), 'resync');
    await waitFor(() => expect(hub.calls).toContain('commLog.list::resync:'));
    await user.type(screen.getByLabelText('Search'), 'checkout');
    await waitFor(() => expect(hub.calls).toContain('commLog.list::resync:checkout'));
  });

  it('clears what was kept', async () => {
    const { hub, user } = await open({ entries: [row(1)] });
    await user.click(await screen.findByRole('button', { name: 'Clear' }));
    await waitFor(() => expect(hub.calls).toContain('commLog.clear'));
    expect(await screen.findByText('Nothing yet')).toBeInTheDocument();
  });

  it('follows live: asks again every couple of seconds while it is on', async () => {
    const { hub } = await open({ entries: [row(1)] });
    await screen.findByText('snapshot ×1 · metrics ×1');
    const before = hub.calls.filter((c) => c.startsWith('commLog.list')).length;
    await act(async () => { await new Promise((r) => setTimeout(r, 2300)); });
    expect(hub.calls.filter((c) => c.startsWith('commLog.list')).length).toBeGreaterThan(before);
  });
});
