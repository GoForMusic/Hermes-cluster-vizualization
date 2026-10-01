import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { renderWithHub } from './render';
import { AuthScreen } from '../features/auth/AuthScreen';

describe('the login screen', () => {
  it('logs in and tells the app, or shows the hub\'s own words when it is refused', async () => {
    const onDone = vi.fn();
    const { hub } = await renderWithHub(<AuthScreen mode="login" onDone={onDone} />, { loaded: false });
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/^Username/), 'admin');
    await user.type(screen.getByLabelText(/^Password/), 'wrong');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('wrong username or password');
    expect(onDone).not.toHaveBeenCalled();

    await user.clear(screen.getByLabelText(/^Password/));
    await user.type(screen.getByLabelText(/^Password/), 'right password');
    await user.click(screen.getByRole('button', { name: 'Log in' }));
    await waitFor(() => expect(onDone).toHaveBeenCalledOnce());
    expect(hub.calls).toEqual(['auth.login:admin', 'auth.login:admin']);
  });

  it('shows the HERMES brand', async () => {
    await renderWithHub(<AuthScreen mode="login" onDone={() => {}} />, { loaded: false });
    expect(screen.getByRole('img', { name: /HERMES/ })).toBeInTheDocument();
  });
});

describe('the first-run screen', () => {
  it('refuses two passwords that differ, without asking the hub', async () => {
    const { hub } = await renderWithHub(<AuthScreen mode="setup" onDone={() => {}} />, { loaded: false });
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/^Username/), 'admin');
    await user.type(screen.getByLabelText(/^Password/), 'a long enough one');
    await user.type(screen.getByLabelText('Repeat password'), 'another long one');
    await user.click(screen.getByRole('button', { name: 'Create admin' }));
    expect(screen.getByRole('alert')).toHaveTextContent('The two passwords do not match.');
    expect(hub.calls).toEqual([]);
  });

  it('creates the admin, with the wallboard public unless that is unticked', async () => {
    const onDone = vi.fn();
    const { hub } = await renderWithHub(<AuthScreen mode="setup" onDone={onDone} />, { loaded: false });
    const user = userEvent.setup();
    await user.type(screen.getByLabelText(/^Username/), 'admin');
    await user.type(screen.getByLabelText(/^Password/), 'a long enough one');
    await user.type(screen.getByLabelText('Repeat password'), 'a long enough one');
    await user.click(screen.getByLabelText(/Allow the wallboard/));
    await user.click(screen.getByRole('button', { name: 'Create admin' }));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(hub.calls).toEqual(['auth.setup:admin:false']);
  });
});
