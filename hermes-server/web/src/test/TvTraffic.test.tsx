import { act, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithHub } from './render';
import { TvScreen } from '../features/tv/TvScreen';

describe('the wallboard traffic list', () => {
  afterEach(() => vi.useRealTimers());

  it('shows nothing until a flows agent reports, then the busiest connections by name', async () => {
    vi.useFakeTimers({ toFake: ['Date'] }); // the test store runs on a fixed clock: the page's clock is the same one
    vi.setSystemTime(new Date(1_700_000_000_000));
    const { store } = await renderWithHub(<TvScreen />);
    expect(screen.queryByText('busiest now')).not.toBeInTheDocument();
    act(() => store.apply({ type: 'flows', source: 's1', flows: [
      { src: 'w', dst: 'h', via: '', port: 80, mbps: 42, external: false },
      { src: '203.0.113.7', dst: 'w', via: '', port: 443, mbps: 3, external: true },
    ] }));
    expect(await screen.findByText('busiest now')).toBeInTheDocument();
    expect(screen.getByText('42 Mb/s')).toBeInTheDocument();
    expect(screen.getByText(/outside · 203\.0\.113\.7/)).toBeInTheDocument();
  });
});
