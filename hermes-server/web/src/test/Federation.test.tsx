import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../app/App';
import { AuthScreen } from '../features/auth/AuthScreen';
import { Settings } from '../features/admin/pages/Settings';
import { Sources } from '../features/admin/pages/Sources';
import type { AuthStatus } from '../generated/AuthStatus';
import type { FederationView } from '../generated/FederationView';
import { HubProvider } from '../state/context';
import { HubStore } from '../state/HubStore';
import { fakeHub } from './fakeHub';
import { renderWithHub } from './render';

const DC2 = { id: 'd1', name: 'dc2', address: 'https://hermes.dc2.example:8765', state: 'connected', info: 'connected', version: '1.0.3', agentAddress: 'http://agents.dc2:8765', sources: 2 };
const withDc2 = (hub: { federation: FederationView }) => { hub.federation = { name: 'dc1', member: null, datacenters: [DC2] }; };

describe('a hub that another hub will manage', () => {
  it('offers it on the first run, and shows the pairing code the hub makes', async () => {
    const onMember = vi.fn();
    const { hub } = await renderWithHub(<AuthScreen mode="setup" onDone={() => {}} onMember={onMember} />, { loaded: false });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Managed by another hub' }));
    expect(screen.queryByLabelText(/^Username/)).not.toBeInTheDocument(); // no admin account is made
    await user.click(screen.getByRole('button', { name: 'Show the pairing code' }));
    await waitFor(() => expect(onMember).toHaveBeenCalledOnce());
    expect(hub.calls).toContain(`federation.enableMember:${location.origin}`);
    expect(onMember.mock.calls[0]![0].code).toBe('ABCD-EFGH-JKMN');
  });

  it('keeps the normal first run as it was when it is not asked for', async () => {
    await renderWithHub(<AuthScreen mode="setup" onDone={() => {}} onMember={() => {}} />, { loaded: false });
    expect(screen.getByRole('heading', { name: 'Create the admin account' })).toBeInTheDocument();
  });
});

describe('what the app shows for a hub in a federation', () => {
  const mount = (auth: Partial<AuthStatus>, prepare?: (hub: ReturnType<typeof fakeHub>) => void) => {
    location.hash = '#/';
    const hub = fakeHub(auth);
    prepare?.(hub);
    const store = new HubStore(hub.client);
    render(<HubProvider client={hub.client} store={store}><App /></HubProvider>);
    return hub;
  };
  afterEach(() => { location.hash = ''; });

  it('shows the pairing code, not the admin form, while it waits for its master', async () => {
    const hub = mount({ setupRequired: true, authenticated: false, member: { state: 'waiting', agentAddress: 'http://hub.dc2:8765', codeExpires: 4_000_000_000_000 } });
    expect(await screen.findByRole('heading', { name: 'Waiting for its master' })).toBeInTheDocument();
    expect(screen.getByText('http://hub.dc2:8765')).toBeInTheDocument();
    // the code is only ever shown when it is made: after a reload there is a button for a new one
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Show a new code' }));
    expect(await screen.findByTestId('pair-code')).toHaveTextContent('WXYZ-2345-6789');
    expect(hub.calls).toContain('federation.newCode');
    expect(screen.queryByRole('heading', { name: 'Create the admin account' })).not.toBeInTheDocument();
  });

  it('is only a notice once the master came: no login, nothing to load', async () => {
    const hub = mount({ setupRequired: false, authenticated: false, member: { state: 'paired', managedBy: 'dc1', datacenter: 'dc2', agentAddress: 'http://hub.dc2:8765' } });
    expect(await screen.findByRole('heading', { name: 'Managed by dc1' })).toBeInTheDocument(); // the master's name, not the datacenter's
    expect(screen.getByText('dc2')).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Log in' })).not.toBeInTheDocument();
    expect(hub.calls).not.toContain('snapshot');
  });
});

describe('Settings → Federation', () => {
  it('connects a datacenter with its pairing code, and lists it', async () => {
    const { hub } = await renderWithHub(<Settings />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add datacenter' }));
    const dialog = within(screen.getByRole('heading', { name: 'Add datacenter', hidden: true }).closest('dialog')!);
    await user.type(dialog.getByLabelText('Name'), 'dc2');
    await user.type(dialog.getByLabelText(/^Address of the child hub/), 'https://hermes.dc2.example:8765');
    await user.type(dialog.getByLabelText('Pairing code'), 'ABCD-EFGH-JKMN');
    await user.click(dialog.getByRole('button', { name: 'Connect' }));
    await waitFor(() => expect(hub.calls).toContain('federation.add:dc2:https://hermes.dc2.example:8765:ABCD-EFGH-JKMN'));
    expect(await screen.findByText('Connected')).toBeInTheDocument();
    expect(screen.getByText(/2 sources|0 sources/)).toBeInTheDocument();
  });

  it('asks before removing a datacenter, and says when it could not be told', async () => {
    const { hub } = await renderWithHub(<Settings />, { prepare: withDc2 });
    const user = userEvent.setup();
    await user.click(await screen.findByRole('button', { name: 'Remove' }));
    expect(screen.getByText('Remove dc2?')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Remove datacenter' }));
    await waitFor(() => expect(hub.calls).toContain('federation.remove:d1'));
    expect(await screen.findByText('dc2 is a hub on its own again.')).toBeInTheDocument();
  });

  it('turns this hub into one that another hub manages, and shows the code', async () => {
    const { hub } = await renderWithHub(<Settings />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: /Let another hub manage this one/ }));
    await user.click(screen.getByRole('button', { name: 'Show the pairing code' }));
    await waitFor(() => expect(hub.calls).toContain(`federation.enableMember:${location.origin}`));
    expect(await screen.findByTestId('pair-code')).toHaveTextContent('ABCD-EFGH-JKMN');
  });

  it('keeps one registry per datacenter: the selector sends the calls there', async () => {
    const { hub } = await renderWithHub(<Settings />, { prepare: withDc2 });
    const user = userEvent.setup();
    const select = await screen.findByLabelText(/^For the agents of/);
    await user.selectOptions(select, 'dc2');
    const address = await screen.findByPlaceholderText('git.example.com');
    await user.clear(address);
    await user.type(address, 'harbor.dc2.example');
    await user.click(screen.getAllByRole('button', { name: 'Test connection' })[0]!);
    await waitFor(() => expect(hub.calls).toContain('registry.test:harbor.dc2.example@d1'));
  });
});

describe('sources on a datacenter', () => {
  it('asks which hub hosts a new source, and adds it there with that hub\'s own address for the agent', async () => {
    const { hub } = await renderWithHub(<Sources />, { prepare: withDc2 });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    const dialog = within(screen.getByRole('heading', { name: 'Add source', hidden: true }).closest('dialog')!);
    await user.selectOptions(await dialog.findByLabelText(/^Hosted by/), 'dc2');
    expect(dialog.getByLabelText(/^Address of the child hub, as the agent reaches it/)).toHaveValue('http://agents.dc2:8765');
    await user.type(dialog.getByLabelText('Name'), 'dc2-docker');
    await user.click(dialog.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(hub.calls).toContain('sources.dc:d1'));
    expect(hub.calls).toContain('sources.add:dc2-docker');
  });

  it('marks a source with its datacenter and sends what is done to it there', async () => {
    const { hub, store } = await renderWithHub(<Sources />, { prepare: withDc2 });
    hub.sources = [{ id: 's9', name: 'dc2-swarm', type: 'Docker Swarm (agent)', endpoint: '', auth: 'Agent token', state: 'connected', info: 'agent reporting', agents: [], canUpgrade: false, dc: 'd1', dcName: 'dc2' }];
    await act(async () => { await store.refreshSources(); });
    expect(screen.getByTitle('Hosted by the datacenter dc2')).toHaveTextContent('dc2');
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    await user.click(screen.getByRole('button', { name: 'Remove source' }));
    expect(hub.calls).toContain('sources.remove:s9@d1');
  });
});

describe('Logs with a datacenter in it', () => {
  it('shows the way a datacenter\'s agent message came: agent → dc2 → this hub, and a local one straight to this hub', async () => {
    const { Logs } = await import('../features/admin/pages/Logs');
    const row = (id: number, via?: string) => ({ id, ts: 1_700_000_000_000, source: 's1', sourceName: 'lab', agent: id === 1 ? 'pod-a' : 'pod-b', dir: 'in', kind: 'batch', summary: 'alive ×1', bytes: 100, ...(via ? { via } : {}) });
    const { hub } = await renderWithHub(<Logs />, { prepare: (h) => { h.commLog = { ...h.commLog, enabled: true, entries: [row(1, 'dc2'), row(2)] }; } });
    await waitFor(() => expect(hub.calls.some((c) => c.startsWith('commLog.list'))).toBe(true));
    expect(await screen.findByText('→ dc2 → hub')).toBeInTheDocument();
    expect(screen.getByText('→ hub')).toBeInTheDocument();
  });
});
