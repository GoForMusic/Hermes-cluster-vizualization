import { it } from 'vitest';
import { screen } from '@testing-library/react';
import { Topology } from '../features/admin/pages/Topology';
import { AdminShellContext } from '../features/admin/adminContext';
import { renderWithHub } from './render';
it('dbg', async () => {
  const { store } = await renderWithHub(<AdminShellContext.Provider value={{ setCursor: () => {} }}><Topology arg="w1" /></AdminShellContext.Provider>, { prepare: (hub) => { hub.sources = [{ id: 'c', name: 'lab', type: 'x', endpoint: '', auth: '', state: 'connected', info: '', agents: [], canUpgrade: false, canLogs: true }]; } });
  console.log('SOURCES', JSON.stringify(store.getState().sources.map((s) => s.id)), 'NODES', [...store.getState().nodes.keys()].join(','));
  console.log(document.querySelector('.detail')?.textContent?.slice(0, 400));
  void screen;
});
