import { screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { Grant } from '../generated/Grant';
import { AdminShell } from '../features/admin/AdminShell';
import { Settings } from '../features/admin/pages/Settings';
import { Alerts } from '../features/admin/pages/Alerts';
import { renderWithHub } from './render';

const auth = (grants: Grant[] | undefined) => ({ setupRequired: false, authenticated: true, publicView: false, username: 'testone', grants });

/** What the policies of a team that was given one cluster come to. */
const team: Grant[] = [{ path: 'clusters/lab', capabilities: ['read'] }];
const owner: Grant[] = [{ path: '*', capabilities: ['read', 'list', 'create', 'update', 'delete'] }];

const nav = () => within(document.querySelector('nav')!).getAllByRole('link').map((a) => a.textContent?.replace(/\d+$/, ''));

describe('what a person is offered, by what their policies let them read', () => {
  it('shows an owner every page', async () => {
    await renderWithHub(<AdminShell page="dashboard" arg="" />, { session: { auth: auth(owner) } });
    expect(nav()).toEqual(['Dashboard', 'Topology', 'Sources', 'Alert rules', 'Alerts', 'Logs', 'Settings']);
  });

  it('shows a team that has a cluster the map and the alerts, and not the sources, the rules and the communication log', async () => {
    await renderWithHub(<AdminShell page="dashboard" arg="" />, { session: { auth: auth(team) } });
    expect(nav()).toEqual(['Dashboard', 'Topology', 'Alerts', 'Settings']);
  });

  it('shows somebody who may see no cluster only their settings', async () => {
    await renderWithHub(<AdminShell page="dashboard" arg="" />, { session: { auth: auth([]) } });
    expect(nav()).toEqual(['Settings']);
  });

  it('does not show a page that was linked to, but is not theirs: the first one they have instead', async () => {
    await renderWithHub(<AdminShell page="sources" arg="" />, { session: { auth: auth(team) } });
    expect(screen.queryByRole('heading', { name: 'Sources' })).not.toBeInTheDocument();
    expect(document.querySelector('nav a.on')?.textContent).toBe('Dashboard');
  });

  it('shows a hub that does not say anything about it everything, as before', async () => {
    await renderWithHub(<AdminShell page="dashboard" arg="" />, { session: { auth: auth(undefined) } });
    expect(nav()).toHaveLength(7);
  });
});

describe('the settings of a team member', () => {
  it('are their profile, and nothing else', async () => {
    await renderWithHub(<Settings arg="" />, { session: { auth: auth(team) } });
    const links = screen.getAllByRole('link').map((a) => a.textContent);
    expect(links).toEqual(['Profile']);
    expect(screen.getByText('Signed in as testone')).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'TV display' })).not.toBeInTheDocument();
  });

  it('are what the owner has: general, users, groups, policies, single sign-on, security and profile', async () => {
    await renderWithHub(<Settings arg="" />, { session: { auth: auth(owner) } });
    expect(screen.getAllByRole('link').map((a) => a.textContent)).toEqual(['General', 'Users', 'Groups', 'Policies', 'Single sign-on', 'Security', 'Profile']);
  });
});

describe('the alerts of a team', () => {
  it('can be looked at but not acknowledged: that is a change', async () => {
    const alert = { id: 1, key: 'k', sev: 'crit' as const, nodeId: 'c:p:w', title: 'Pod crashing', detail: '', ts: 1_700_000_000_000, resolvedTs: null, ack: false, snapshot: '' };
    await renderWithHub(<Alerts />, { prepare: (hub) => { hub.alerts = [alert]; }, session: { auth: auth(team) } });
    expect(await screen.findByText('Pod crashing')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Ack' })).not.toBeInTheDocument();
  });

  it('can be acknowledged by who may', async () => {
    const alert = { id: 1, key: 'k', sev: 'crit' as const, nodeId: 'c:p:w', title: 'Pod crashing', detail: '', ts: 1_700_000_000_000, resolvedTs: null, ack: false, snapshot: '' };
    await renderWithHub(<Alerts />, { prepare: (hub) => { hub.alerts = [alert]; }, session: { auth: auth(owner) } });
    expect(await screen.findByRole('button', { name: 'Ack' })).toBeInTheDocument();
  });
});
