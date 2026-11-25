import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderWithHub } from './render';
import { Settings } from '../features/admin/pages/Settings';
import { AuthScreen } from '../features/auth/AuthScreen';

describe('settings → security', () => {
  it('shows how long a login lasts and saves a change', async () => {
    const user = userEvent.setup();
    const { hub } = await renderWithHub(<Settings arg="security" />);
    const hours = await screen.findByLabelText(/Log out after \(hours\)/);
    await waitFor(() => expect(hours).toHaveValue(336));
    expect(screen.getByLabelText(/being away/)).toHaveValue(0);
    await user.clear(hours);
    await user.type(hours, '8');
    await user.clear(screen.getByLabelText(/being away/));
    await user.type(screen.getByLabelText(/being away/), '30');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('Saved.')).toBeInTheDocument();
    expect(hub.calls).toContain('auth.sessionPolicy:8:30');
  });

  it('refuses what the hub would refuse before asking it', async () => {
    const user = userEvent.setup();
    const { hub } = await renderWithHub(<Settings arg="security" />);
    const idle = await screen.findByLabelText(/being away/);
    await waitFor(() => expect(screen.getByLabelText(/Log out after \(hours\)/)).toHaveValue(336));
    await user.clear(idle);
    await user.type(idle, '3');
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText(/0 \(never\) or between 5 and 1440/)).toBeInTheDocument();
    expect(hub.calls.some((c) => c.startsWith('auth.sessionPolicy'))).toBe(false);
  });

  it('says so when the hub refuses', async () => {
    const user = userEvent.setup();
    const { hub } = await renderWithHub(<Settings arg="security" />);
    await waitFor(() => expect(screen.getByLabelText(/Log out after \(hours\)/)).toHaveValue(336));
    hub.accessError = 'not allowed: update on security';
    await user.click(screen.getByRole('button', { name: 'Save' }));
    expect(await screen.findByText('not allowed: update on security')).toBeInTheDocument();
  });
});

describe('the login screen after a login ended', () => {
  it('says why the person is asked again', async () => {
    await renderWithHub(<AuthScreen mode="login" expired onDone={() => {}} />);
    expect(screen.getByText(/your login ended/)).toBeInTheDocument();
  });
  it('says nothing on a first visit', async () => {
    await renderWithHub(<AuthScreen mode="login" onDone={() => {}} />);
    expect(screen.queryByText(/your login ended/)).not.toBeInTheDocument();
  });
});
