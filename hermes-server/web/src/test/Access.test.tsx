import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { GroupView } from '../generated/GroupView';
import type { UserView } from '../generated/UserView';
import { renderWithHub } from './render';
import { Settings } from '../features/admin/pages/Settings';

const alice: UserView = { id: 2, username: 'alice', email: 'alice@example.com', role: 'admin', disabled: false, breakGlass: false, created: 2, groups: [20] };
const ops: GroupView = { id: 20, name: 'ops', description: 'on call', created: 2, members: [2] };

const open = (section = 'users') => renderWithHub(<Settings arg={section} />, { prepare: (hub) => { hub.users = [...hub.users, alice]; hub.groups = [ops]; } });

describe('the access page', () => {
  it('lists the users with their groups and marks the break-glass account', async () => {
    await open();
    const row = (await screen.findByText('alice')).closest('tr')!;
    expect(within(row).getByText('alice@example.com')).toBeInTheDocument();
    expect(within(row).getByText('ops')).toBeInTheDocument();
    const admin = screen.getByText('admin').closest('tr')!;
    expect(within(admin).getByText('Break-glass')).toBeInTheDocument();
  });

  it('adds a user, with the email and the break-glass flag as typed', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    await screen.findByText('alice');
    await user.click(screen.getByRole('button', { name: '+ Add user' }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText('Username'), 'bobby');
    await user.type(within(dialog).getByLabelText('Email (optional)'), 'bobby@example.com');
    await user.type(within(dialog).getByLabelText(/^Password/), 'a long enough password');
    await user.click(within(dialog).getByLabelText(/Break-glass/));
    await user.click(within(dialog).getByRole('button', { name: 'Add user' }));
    expect(await screen.findByText('bobby')).toBeInTheDocument();
    expect(hub.calls).toContain('access.createUser:bobby:bobby@example.com:true');
  });

  it('shows the reason when the hub refuses', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    await screen.findByText('alice');
    hub.accessError = 'that name is already in use';
    await user.click(screen.getByRole('button', { name: '+ Add user' }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText('Username'), 'alice');
    await user.type(within(dialog).getByLabelText(/^Password/), 'a long enough password');
    await user.click(within(dialog).getByRole('button', { name: 'Add user' }));
    expect(await within(dialog).findByText('that name is already in use')).toBeInTheDocument();
  });

  it('sends only what changed when a user is edited: disabling and a new password', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    const row = (await screen.findByText('alice')).closest('tr')!;
    await user.click(within(row).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByLabelText(/Disabled/));
    await user.type(within(dialog).getByLabelText(/Set a new password/), 'another long password');
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('access.updateUser:2:{"disabled":true,"password":"another long password"}'));
    expect(await screen.findByText('Disabled')).toBeInTheDocument();
  });

  it('does not offer to disable or reset yourself', async () => {
    const user = userEvent.setup();
    await open(); // the session is `admin`
    const row = (await screen.findByText('admin')).closest('tr')!;
    await user.click(within(row).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByLabelText(/Disabled/)).toBeDisabled();
    expect(within(dialog).queryByLabelText(/Set a new password/)).not.toBeInTheDocument();
    expect(within(dialog).getByText(/change your password in Profile/)).toBeInTheDocument();
  });

  it('asks before deleting a user', async () => {
    const user = userEvent.setup();
    const { hub } = await open();
    const row = (await screen.findByText('alice')).closest('tr')!;
    await user.click(within(row).getByRole('button', { name: 'Delete' }));
    expect(hub.calls).not.toContain('access.deleteUser:2');
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete user' }));
    await waitFor(() => expect(hub.calls).toContain('access.deleteUser:2'));
    await waitFor(() => expect(screen.queryByText('alice')).not.toBeInTheDocument());
  });

  it('has a link to each section of the settings', async () => {
    const user = userEvent.setup();
    await open();
    await screen.findByText('alice');
    await user.click(screen.getByRole('link', { name: 'Groups' }));
    expect(location.hash).toBe('#/admin/settings/groups');
  });

  it('creates a group and sets its members', async () => {
    const user = userEvent.setup();
    const { hub } = await open('groups');
    expect(await screen.findByText('on call')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '+ Add group' }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText('Name'), 'dev');
    await user.click(within(dialog).getByLabelText('alice'));
    await user.click(within(dialog).getByRole('button', { name: 'Add group' }));
    await waitFor(() => expect(hub.calls).toContain('access.createGroup:dev'));
    await waitFor(() => expect(hub.calls).toContain('access.setMembers:21:2'));
    expect(await screen.findByText('dev')).toBeInTheDocument();
  });

  it('deletes a group after asking', async () => {
    const user = userEvent.setup();
    const { hub } = await open('groups');
    const row = (await screen.findByText('ops')).closest('tr')!;
    await user.click(within(row).getByRole('button', { name: 'Delete' }));
    await user.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Delete group' }));
    await waitFor(() => expect(hub.calls).toContain('access.deleteGroup:20'));
  });
});

describe('the settings sections', () => {
  it('shows General by default and Profile with the password form', async () => {
    const { unmount } = await renderWithHub(<Settings arg="" />);
    expect(screen.getByRole('heading', { name: 'TV display' })).toBeInTheDocument();
    unmount();
    await renderWithHub(<Settings arg="profile" />);
    expect(screen.getByText('Signed in as admin')).toBeInTheDocument();
    expect(screen.getByLabelText('Current password')).toBeInTheDocument();
  });
});
