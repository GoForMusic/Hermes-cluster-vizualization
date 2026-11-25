import { act, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { AdminShellContext } from '../features/admin/adminContext';
import { Topology } from '../features/admin/pages/Topology';
import { renderWithHub } from './render';

const source = (over: { canLogs: boolean; dc?: string; dcName?: string }) => ({
  id: 'c', name: 'lab', type: 'Kubernetes (agent)', endpoint: '', auth: '', state: 'connected', info: '', agents: [], canUpgrade: false, ...over,
});

const page = (over: { canLogs: boolean; dc?: string; dcName?: string }) =>
  renderWithHub(<AdminShellContext.Provider value={{ setCursor: () => {} }}><Topology arg="w" /></AdminShellContext.Provider>, {
    prepare: (hub) => { hub.sources = [source(over)]; },
  });

describe('the way into the log panel, in the details of a pod', () => {
  it('is a button when the source reads logs', async () => {
    const { store } = await page({ canLogs: true });
    await act(async () => { await store.refreshSources(); });
    expect(await screen.findByRole('button', { name: 'Open the log' })).toBeInTheDocument();
  });

  it('sends a cluster of a datacenter to that hub: a master does not carry the stream', async () => {
    const { store } = await page({ canLogs: true, dc: 'd1', dcName: 'dc2' });
    await act(async () => { await store.refreshSources(); });
    expect(await screen.findByText(/belongs to the datacenter dc2: read its logs on that hub/)).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Open the log' })).not.toBeInTheDocument();
  });
});
