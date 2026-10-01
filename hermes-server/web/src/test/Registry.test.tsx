import { act, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderWithHub } from './render';
import { Settings } from '../features/admin/pages/Settings';
import { Sources } from '../features/admin/pages/Sources';
import { SetupRegistry } from '../features/registry/SetupRegistry';

describe('the registry step of the setup', () => {
  it('saves the registry and moves on, or is skipped', async () => {
    let done = 0;
    const { hub } = await renderWithHub(<SetupRegistry onDone={() => { done += 1; }} />);
    const user = userEvent.setup();
    expect(screen.getByText('Step 2 of 2')).toBeInTheDocument();
    await user.type(await screen.findByPlaceholderText('git.example.com'), 'git.example.com');
    await user.click(screen.getByRole('button', { name: 'Save and continue' }));
    await waitFor(() => expect(done).toBe(1));
    expect(hub.calls).toContain('registry.save:git.example.com:none:-');
    await user.click(screen.getByRole('button', { name: 'Skip for now' }));
    expect(done).toBe(2);
  });

  it('asks for the address instead of saving nothing', async () => {
    const { hub } = await renderWithHub(<SetupRegistry onDone={() => {}} />);
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Save and continue' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Type the registry address');
    expect(hub.calls.some((c) => c.startsWith('registry.save'))).toBe(false);
  });
});

describe('Settings → Registry', () => {
  it('tests what is typed before it is saved, and sends the password only when it is typed', async () => {
    const { hub } = await renderWithHub(<Settings />, {
      prepare: (h) => { h.registry = { ...h.registry, url: 'harbor.example.com', project: 'acm', auth: 'basic', username: 'robot', hasSecret: true }; },
    });
    const user = userEvent.setup();
    expect(await screen.findByDisplayValue('harbor.example.com')).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/saved \(type to change\)/)).toHaveValue('');
    await user.click(screen.getByRole('button', { name: 'Test connection' }));
    expect(await screen.findByText(/connected · versions: 1\.0\.1, 1\.0\.0/)).toBeInTheDocument();
    expect(hub.calls).toContain('registry.test:harbor.example.com');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('registry.save:harbor.example.com:basic:-')); // the stored password is kept
    await user.type(screen.getByPlaceholderText(/saved \(type to change\)/), 'new');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('registry.save:harbor.example.com:basic:new'));
  });
});

describe('Add source with a registry', () => {
  it('offers the versions in the registry and sends the chosen one', async () => {
    const { hub } = await renderWithHub(<Sources />);
    hub.registry = { ...hub.registry, url: 'git.example.com' };
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    const select = await screen.findByLabelText(/Agent version/);
    await waitFor(() => expect(screen.getByRole('option', { name: '1.0.1 (newest)' })).toBeInTheDocument());
    await user.selectOptions(select, '1.0.0');
    await user.type(screen.getByPlaceholderText('e.g. lab-k3s'), 'prod');
    await act(async () => { await user.click(screen.getByRole('button', { name: 'Add' })); });
    await waitFor(() => expect(hub.calls).toContain('sources.version:1.0.0'));
  });

  it('says so when no registry is set up, and sends no version', async () => {
    const { hub } = await renderWithHub(<Sources />);
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: '+ Add source' }));
    expect(await screen.findByText(/No registry is set up/)).toBeInTheDocument();
    await user.type(screen.getByPlaceholderText('e.g. lab-k3s'), 'prod');
    await act(async () => { await user.click(screen.getByRole('button', { name: 'Add' })); });
    await waitFor(() => expect(hub.calls).toContain('sources.version:'));
  });
});
