import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { SourceView } from '../generated/SourceView';
import { renderWithHub } from './render';
import { Sources } from '../features/admin/pages/Sources';

const source = (over: Partial<SourceView> = {}): SourceView => ({
  id: 's1', name: 'lab-k3s', type: 'Kubernetes (agent)', endpoint: '', auth: 'Agent token', state: 'connected', info: 'agent reporting · 12 nodes',
  agents: [], expectedAgent: '1.2.3', canUpgrade: false, ...over,
});
const agent = (over: Partial<SourceView['agents'][number]> = {}) => ({ id: 'pod-a', version: '1.2.3', collector: 'kubernetes', host: '', seenAgo: 2, outdated: false, protocolOutdated: false, ...over });

describe('the sources page', () => {
  it('shows each source with its state and the version its agents run', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    hub.sources = [source({ agents: [agent()] })];
    await act(async () => { await store.refreshSources(); });
    expect(screen.getByText('lab-k3s')).toBeInTheDocument();
    expect(screen.getByText('Connected')).toBeInTheDocument();
    expect(screen.getByText(/Agent v1\.2\.3/)).toBeInTheDocument();
    expect(screen.queryByText(/update → v/)).not.toBeInTheDocument();
  });

  it('marks agents that are behind the version the manifests install', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    hub.sources = [source({ agents: [agent({ version: '1.0.0', outdated: true }), agent({ id: 'pod-b', version: '1.2.3' })] })];
    await act(async () => { await store.refreshSources(); });
    expect(screen.getByText(/Agent v1\.0\.0, v1\.2\.3 · 2 instances/)).toBeInTheDocument();
    expect(screen.getByText(/update → v1\.2\.3/)).toBeInTheDocument();
  });

  it('warns when an agent speaks an older wire protocol than the hub', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    hub.sources = [source({ agents: [agent({ protocolOutdated: true })] })];
    await act(async () => { await store.refreshSources(); });
    expect(screen.getByText(/old protocol/)).toBeInTheDocument();
  });

  it('does not count an agent that was replaced and has not been heard of for a while', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    hub.sources = [source({ agents: [agent({ version: '0.9.0', seenAgo: 900, outdated: true })] })];
    await act(async () => { await store.refreshSources(); });
    expect(screen.queryByText(/Agent v/)).not.toBeInTheDocument();
  });

  it('asks in a dialog before removing a source, and Cancel leaves it alone', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    hub.sources = [source()];
    await act(async () => { await store.refreshSources(); });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    expect(screen.getByText('Remove lab-k3s?')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(hub.calls).not.toContain('sources.remove:s1');
    await user.click(screen.getByRole('button', { name: 'Remove' }));
    await user.click(screen.getByRole('button', { name: 'Remove source' }));
    expect(hub.calls).toContain('sources.remove:s1');
  });

  it('adds a source and shows the manifest that installs its agent', async () => {
    const { hub } = await renderWithHub(<Sources />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    const dialog = screen.getAllByRole('dialog', { hidden: true })[0]!;
    await user.type(within(dialog).getByLabelText('Name'), 'lab-swarm');
    await user.click(within(dialog).getByRole('button', { name: 'Add' }));
    expect(await screen.findByText(/Install the agent for “lab-swarm”/, {}, { timeout: 2000 })).toBeInTheDocument();
    expect(hub.calls).toContain('sources.add:lab-swarm');
    expect(screen.getByText('yaml')).toBeInTheDocument();
  });

  it('offers who-talks-to-whom for Kubernetes only, off unless it is ticked', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    void store;
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    const dialog = screen.getAllByRole('dialog', { hidden: true })[0]!;
    await user.selectOptions(within(dialog).getByLabelText('Type'), 'Docker Swarm (agent)');
    expect(within(dialog).queryByLabelText(/Show who talks to whom/)).not.toBeInTheDocument();
    await user.selectOptions(within(dialog).getByLabelText('Type'), 'Kubernetes (agent)');
    const box = within(dialog).getByLabelText(/Show who talks to whom/);
    expect(box).not.toBeChecked();
    await user.type(within(dialog).getByLabelText('Name'), 'lab-k8s');
    await user.click(box);
    await user.click(within(dialog).getByRole('button', { name: 'Add' }));
    expect(await screen.findByText(/Install the agent for “lab-k8s”/, {}, { timeout: 2000 })).toBeInTheDocument();
    expect(hub.calls).toContain('sources.flows:true');
  });

  it('says so when there is nothing yet', async () => {
    await renderWithHub(<Sources />);
    expect(screen.getByText('No sources yet')).toBeInTheDocument();
  });
});

describe('changing the agent version', () => {
  const versions = ['1.0.2', '1.0.1', '1.0.0'];
  const open = async (over: Partial<SourceView> = {}) => {
    const rig = await renderWithHub(<Sources />, { prepare: (h) => { h.registryTest = { ok: true, message: 'connected', versions }; h.registry = { ...h.registry, url: 'git.example.com' }; } });
    rig.hub.sources = [source({ agents: [agent({ version: '1.0.1' })], canUpgrade: true, ...over })];
    await act(async () => { await rig.store.refreshSources(); });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Change agent version' }));
    return { ...rig, user };
  };

  it('has the button on the left of Remove, lists the registry versions and marks the one running', async () => {
    const { user } = await open();
    const buttons = screen.getAllByRole('button').map((b) => b.textContent);
    expect(buttons.indexOf('Change agent version')).toBeLessThan(buttons.indexOf('Remove'));
    expect(await screen.findByText('v1.0.2')).toBeInTheDocument();
    expect(screen.getByText('newest')).toBeInTheDocument();
    expect(screen.getByText('running now')).toBeInTheDocument();
    expect(screen.getByText(/Running now: v1\.0\.1/)).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    expect(screen.getByText('Choose a version.')).toBeInTheDocument();
  });

  it('sends the chosen version, older ones too (a rollback)', async () => {
    const { hub, user } = await open();
    await user.click(await screen.findByLabelText(/v1\.0\.0/));
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(hub.calls).toContain('sources.upgrade:s1:1.0.0'));
  });

  it('shows why the hub refused', async () => {
    const { hub, user } = await open();
    hub.upgradeError = 'no connected agent of this source was installed to upgrade itself';
    await user.click(await screen.findByLabelText(/v1\.0\.2/));
    await user.click(screen.getByRole('button', { name: 'Apply' }));
    expect(await screen.findByText(/no connected agent of this source/)).toBeInTheDocument();
  });

  it('says what to do when the agents were installed read-only, and cannot apply', async () => {
    const { hub } = await open({ canUpgrade: false });
    expect(await screen.findByText(/Add the source again with “Allow upgrades from the dashboard” ticked/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Apply' })).toBeDisabled();
    expect(hub.calls.some((c) => c.startsWith('sources.upgrade'))).toBe(false);
  });

  it('follows the change on the card: pending, done, failed with the reason', async () => {
    const { hub, store } = await renderWithHub(<Sources />);
    const show = async (upgrade: SourceView['upgrade']) => { hub.sources = [source({ upgrade })]; await act(async () => { await store.refreshSources(); }); };
    await show({ version: '1.0.2', state: 'pending', message: '' });
    expect(screen.getByText('Changing to v1.0.2…')).toBeInTheDocument();
    await show({ version: '1.0.2', state: 'done', message: '' });
    expect(screen.getByText('Now v1.0.2')).toBeInTheDocument();
    await show({ version: '1.0.2', state: 'failed', message: 'the node agents did not come up' });
    expect(screen.getByText('Change to v1.0.2 failed')).toBeInTheDocument();
    expect(screen.getByText('the node agents did not come up')).toBeInTheDocument();
  });

  it('asks in Add source whether the agent may upgrade itself, on by default', async () => {
    const { hub } = await renderWithHub(<Sources />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    const box = await screen.findByRole('checkbox', { name: /Allow upgrades from the dashboard/ });
    expect(box).toBeChecked();
    await user.click(box);
    await user.type(screen.getByPlaceholderText('e.g. lab-k3s'), 'prod');
    await act(async () => { await user.click(screen.getByRole('button', { name: 'Add' })); });
    await waitFor(() => expect(hub.calls).toContain('sources.upgrades:false'));
  });
});
