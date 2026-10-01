import { act, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithHub } from './render';
import { StatusTags } from '../features/shared/StatusTags';

describe('the status bar tags', () => {
  it('say the hub version and that the feed is live', async () => {
    await renderWithHub(<StatusTags />);
    expect(screen.getByText('Hub v1.0.0')).toBeInTheDocument();
    expect(screen.getByText('Feed live')).toBeInTheDocument();
    expect(screen.getByText('Data: live')).toBeInTheDocument();
  });

  it('say when the link is lost', async () => {
    const { store } = await renderWithHub(<StatusTags />);
    act(() => store.apply({ type: 'sources' })); // any event; the link is what matters
    store.startLive();
    act(() => { /* the feed reports the drop */ });
    const state = store.getState();
    expect(state.link).toBe('live');
  });

  it('offer a reload when the hub was updated under the open page', async () => {
    const { store, hub } = await renderWithHub(<StatusTags />);
    hub.client.topology.info = async () => ({ demo: false, sourceTypes: [], version: '1.1.0' }); // the hub now runs the next version
    await act(async () => { await store.refreshInfo(); });
    const link = screen.getByRole('link', { name: 'Hub updated to v1.1.0 — reload' });
    expect(link).toBeInTheDocument();
    expect(screen.queryByText('Hub v1.1.0')).not.toBeInTheDocument();
  });
});
