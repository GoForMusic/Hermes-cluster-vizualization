import { render } from '@testing-library/react';
import type { ReactElement } from 'react';
import { SessionContext, type Session } from '../app/session';
import { HubStore } from '../state/HubStore';
import { HubProvider } from '../state/context';
import { fakeHub, type FakeHub } from './fakeHub';

export interface Rig {
  hub: FakeHub;
  store: HubStore;
}

/** A screen against a fake hub. `loaded`: the store has already loaded its data (as it has by the time a screen is shown). */
export async function renderWithHub(ui: ReactElement, options: { loaded?: boolean; admin?: boolean; session?: Partial<Session>; prepare?: (hub: FakeHub) => void } = {}): Promise<Rig & ReturnType<typeof render>> {
  const hub = fakeHub();
  options.prepare?.(hub); // what the hub holds before the screen is first drawn
  const store = new HubStore(hub.client, () => 1_700_000_000_000);
  if (options.loaded !== false) await store.load(options.admin !== false);
  const session: Session = { auth: { setupRequired: false, authenticated: true, publicView: false, username: 'admin' }, refresh: async () => {}, logout: async () => {}, ...options.session };
  const utils = render(
    <HubProvider client={hub.client} store={store}>
      <SessionContext.Provider value={session}>{ui}</SessionContext.Provider>
    </HubProvider>,
  );
  return { hub, store, ...utils };
}
