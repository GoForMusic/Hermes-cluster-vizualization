import { render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { HubStore } from '../state/HubStore';
import { HubProvider } from '../state/context';
import { fakeHub } from './fakeHub';
import type { AuthStatus } from '../generated/AuthStatus';
import { App } from '../app/App';

function mount(auth: Partial<AuthStatus>, hash: string) {
  location.hash = hash;
  const hub = fakeHub(auth);
  const store = new HubStore(hub.client);
  render(<HubProvider client={hub.client} store={store}><App /></HubProvider>);
  return { hub, store };
}

afterEach(() => { location.hash = ''; });

describe('what the app shows first', () => {
  it('asks for the admin to be created on the first visit', async () => {
    mount({ setupRequired: true, authenticated: false }, '#/');
    expect(await screen.findByRole('heading', { name: 'Create the admin account' })).toBeInTheDocument();
  });

  it('asks for a login when the wallboard is private', async () => {
    const { hub } = mount({ authenticated: false, publicView: false }, '#/tv');
    expect(await screen.findByRole('heading', { name: 'Log in' })).toBeInTheDocument();
    expect(hub.calls).not.toContain('snapshot'); // nothing is loaded for someone who may not see it
  });

  it('shows the wallboard without a login when it is public, and never loads source details', async () => {
    const { hub } = mount({ authenticated: false, publicView: true }, '#/tv');
    expect(await screen.findByRole('button', { name: 'All clusters' })).toBeInTheDocument();
    expect(hub.calls).toContain('snapshot');
    expect(hub.calls).not.toContain('sources.list');
  });

  it('still asks for a login at the admin panel when the wallboard is public', async () => {
    mount({ authenticated: false, publicView: true }, '#/admin');
    expect(await screen.findByRole('heading', { name: 'Log in' })).toBeInTheDocument();
  });

  it('shows the landing page with the version of the hub', async () => {
    mount({ authenticated: true }, '#/');
    expect(await screen.findByText('HERMES v1.0.0')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /TV display/ })).toHaveAttribute('href', '#/tv');
  });

  it('opens the admin panel for an admin, on the page the address names', async () => {
    mount({ authenticated: true }, '#/admin/rules');
    expect(await screen.findByRole('heading', { name: 'Alert rules' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Alert rules' })).toHaveClass('on');
    expect(screen.getByText('Log out (admin)')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('Hub v1.0.0')).toBeInTheDocument());
  });

  it('shows what went wrong when the hub cannot be reached, with a way to try again', async () => {
    location.hash = '#/';
    const hub = fakeHub();
    hub.client.auth.status = async () => { throw new Error('connection refused'); };
    render(<HubProvider client={hub.client} store={new HubStore(hub.client)}><App /></HubProvider>);
    expect(await screen.findByText('connection refused')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument();
  });
});
