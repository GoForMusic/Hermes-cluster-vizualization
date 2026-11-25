import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { GroupView } from '../generated/GroupView';
import type { ProviderView } from '../generated/ProviderView';
import { renderWithHub } from './render';
import { Settings } from '../features/admin/pages/Settings';
import { AuthScreen } from '../features/auth/AuthScreen';

const entra: ProviderView = {
  id: 'microsoft-entra-id', name: 'Microsoft Entra ID', issuer: 'https://login.microsoftonline.com/t-1/v2.0', clientId: 'app-1', hasSecret: true,
  scopes: 'openid profile email', groupsClaim: 'groups', allowedDomains: '', enabled: true, autoCreate: true, linkExisting: true, trustEmail: true, enforce: false,
  defaultGroup: null, groupMap: [], redirectUri: 'https://hub.example.com/api/auth/sso/microsoft-entra-id/callback',
};
const owners: GroupView = { id: 20, name: 'owners', description: '', created: 1, members: [], system: false, policies: [] };

const open = () => renderWithHub(<Settings arg="sso" />, { prepare: (hub) => { hub.providers = [entra]; hub.groups = [owners]; } });

describe('settings → single sign-on', () => {
  it('lists the providers and says where this hub must be registered', async () => {
    const user = userEvent.setup();
    await open();
    const row = (await screen.findByText('Microsoft Entra ID')).closest('tr')!;
    expect(within(row).getByText('On')).toBeInTheDocument();
    await user.click(within(row).getByRole('button', { name: 'Edit' }));
    expect(within(screen.getByRole('dialog')).getByText(entra.redirectUri)).toBeInTheDocument();
  });

  it('adds a provider with a group mapping, and fills in Entra', async () => {
    const user = userEvent.setup();
    const { hub } = await renderWithHub(<Settings arg="sso" />, { prepare: (h) => { h.groups = [owners]; } });
    await user.click(await screen.findByRole('button', { name: '+ Add provider' }));
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Fill in for Microsoft Entra ID' }));
    expect(within(dialog).getByLabelText('Issuer address')).toHaveValue('https://login.microsoftonline.com/TENANT-ID/v2.0');
    await user.type(within(dialog).getByLabelText(/Client \(application\) ID/), 'app-9');
    await user.type(within(dialog).getByLabelText(/^Client secret/), 'shh');
    await user.click(within(dialog).getByRole('button', { name: '+ Add mapping' }));
    await user.type(within(dialog).getByLabelText('Provider group 1'), 'group-guid-1');
    await user.click(within(dialog).getByRole('button', { name: 'Add provider' }));
    await waitFor(() => expect(hub.calls).toContain('sso.create:Microsoft Entra ID:https://login.microsoftonline.com/TENANT-ID/v2.0:secret:1'));
  });

  it('tests the address before it is saved, and shows what the hub found', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    await user.click(within((await screen.findByText('Microsoft Entra ID')).closest('tr')!).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    hub.providerTest = { ok: false, message: 'cannot read the address' };
    await user.click(within(dialog).getByRole('button', { name: 'Test connection' }));
    expect(await within(dialog).findByText('cannot read the address')).toBeInTheDocument();
    expect(hub.calls).toContain(`sso.test:${entra.issuer}`);
  });

  it('keeps the saved secret when the field is left empty', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    await user.click(within((await screen.findByText('Microsoft Entra ID')).closest('tr')!).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByLabelText(/Passwords for break-glass accounts only/));
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('sso.update:microsoft-entra-id:Microsoft Entra ID:nosecret:true'));
  });

  it('asks before deleting', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    await user.click(within((await screen.findByText('Microsoft Entra ID')).closest('tr')!).getByRole('button', { name: 'Delete' }));
    expect(hub.calls).not.toContain('sso.remove:microsoft-entra-id');
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete provider' }));
    await waitFor(() => expect(hub.calls).toContain('sso.remove:microsoft-entra-id'));
  });
});

describe('the login screen with single sign-on', () => {
  it('offers each provider, and the local account (break-glass) below', async () => {
    await renderWithHub(<AuthScreen mode="login" onDone={() => {}} />, {
      prepare: (hub) => {
        const status = hub.client.auth.status;
        hub.client.auth.status = async () => ({ ...(await status()), authenticated: false, sso: [{ id: 'microsoft-entra-id', name: 'Microsoft Entra ID' }] });
      },
    });
    const link = await screen.findByRole('link', { name: /Sign in with Microsoft Entra ID/ });
    expect(link).toHaveAttribute('href', '/api/auth/sso/microsoft-entra-id/login');
    expect(screen.getByText(/or with a local account/)).toBeInTheDocument();
    expect(screen.getByLabelText('Username')).toBeInTheDocument();
  });

  it('shows why the person was sent back, once', async () => {
    history.replaceState(null, '', '/?sso_error=this%20account%20is%20disabled#/admin');
    await renderWithHub(<AuthScreen mode="login" onDone={() => {}} />);
    expect(await screen.findByText('this account is disabled')).toBeInTheDocument();
    expect(location.search).toBe('');
  });
});
