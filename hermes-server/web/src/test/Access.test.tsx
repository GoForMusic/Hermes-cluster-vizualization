import { screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { GroupView } from '../generated/GroupView';
import type { PolicyView } from '../generated/PolicyView';
import type { UserView } from '../generated/UserView';
import { renderWithHub } from './render';
import { Settings } from '../features/admin/pages/Settings';

const alice: UserView = { id: 2, username: 'alice', email: 'alice@example.com', role: 'admin', disabled: false, breakGlass: false, created: 2, groups: [20] };
const ops: GroupView = { id: 20, name: 'ops', description: 'on call', created: 2, members: [2], system: false, policies: [30] };
const everyone: GroupView = { id: 21, name: 'everyone', description: 'all of them', created: 1, members: [1, 2], system: true, policies: [] };
const readOnly: PolicyView = { id: 30, name: 'read-only', description: 'look', hcl: 'path "*" {\n  capabilities = ["read"]\n}\n', builtin: true, created: 1, groups: [20] };

/** The table row of something named `name`: the same word can also be in the text above the table. */
const rowOf = (name: string): HTMLElement => screen.getAllByText(name).map((e) => e.closest('tr')).find((r): r is HTMLTableRowElement => r !== null)!;

const open = (section = 'users') => renderWithHub(<Settings arg={section} />, { prepare: (hub) => { hub.users = [...hub.users, alice]; hub.groups = [ops, everyone]; hub.policies = [readOnly]; } });

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
    await user.click(within(dialog).getByRole('combobox', { name: 'Search members to add' }));
    await user.click(within(dialog).getByRole('option', { name: /alice/ }));
    expect(within(dialog).getByRole('button', { name: 'Remove alice' })).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Add group' }));
    await waitFor(() => expect(hub.calls).toContain('access.createGroup:dev'));
    await waitFor(() => expect(hub.calls).toContain('access.setMembers:22:2'));
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

describe('search and pages', () => {
  const many = Array.from({ length: 23 }, (_, i): UserView => ({ id: 100 + i, username: `user${String(i).padStart(2, '0')}`, email: i === 7 ? 'special@example.com' : '', role: 'admin', disabled: false, breakGlass: false, created: 2, groups: [] }));
  const openMany = () => renderWithHub(<Settings arg="users" />, { prepare: (hub) => { hub.users = [...hub.users, ...many]; } });

  it('shows ten at a time and moves between pages', async () => {
    const user = userEvent.setup();
    await openMany();
    expect(await screen.findByText('1–10 of 24')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '← Previous' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Next →' }));
    expect(screen.getByText('11–20 of 24')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Next →' }));
    expect(screen.getByText('21–24 of 24')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next →' })).toBeDisabled();
  });

  it('searches by name or email and starts again from the first page', async () => {
    const user = userEvent.setup();
    await openMany();
    await screen.findByText('1–10 of 24');
    await user.click(screen.getByRole('button', { name: 'Next →' }));
    await user.type(screen.getByRole('searchbox', { name: 'Search users' }), 'special@');
    expect(await screen.findByText('1–1 of 1')).toBeInTheDocument();
    expect(screen.getByText('user07')).toBeInTheDocument();
    await user.clear(screen.getByRole('searchbox', { name: 'Search users' }));
    await user.type(screen.getByRole('searchbox', { name: 'Search users' }), 'nobody-called-this');
    expect(await screen.findByText('nothing found')).toBeInTheDocument();
  });
});

describe('groups and policies', () => {
  it('shows what a group gives, and that everyone has no members to choose', async () => {
    const user = userEvent.setup();
    await open('groups');
    const row = (await screen.findByText('ops')).closest('tr')!;
    expect(within(row).getByText('read-only')).toBeInTheDocument();
    const every = rowOf('everyone');
    expect(within(every).getByText('every user')).toBeInTheDocument();
    expect(within(every).queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    await user.click(within(every).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByLabelText('Name')).toBeDisabled();
    expect(within(dialog).queryByText('alice')).not.toBeInTheDocument();
    expect(within(dialog).queryByRole('combobox', { name: 'Search members to add' })).not.toBeInTheDocument();
  });

  it('finds what to add by typing, and takes it away again with the ×', async () => {
    const user = userEvent.setup();
    const { hub } = await renderWithHub(<Settings arg="groups" />, { prepare: (h) => { h.users = [...h.users, alice]; h.groups = [ops]; h.policies = [readOnly, { ...readOnly, id: 31, name: 'source-editor', description: 'edit sources', groups: [] }, { ...readOnly, id: 32, name: 'freeze', description: 'deny prod', groups: [] }]; } });
    await user.click(within(await screen.findByText('ops').then((e) => e.closest('tr')!)).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    // read-only is already given: it is a chip, not a result
    expect(within(dialog).getByRole('button', { name: 'Remove read-only' })).toBeInTheDocument();
    await user.type(within(dialog).getByRole('combobox', { name: 'Search policies to add' }), 'edit');
    const options = within(dialog).getAllByRole('option');
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveTextContent('source-editor');
    await user.click(options[0]!);
    await user.click(within(dialog).getByRole('button', { name: 'Remove read-only' }));
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('access.setGroupPolicies:20:31'));
  });

  it('gives a policy to a group', async () => {
    const user = userEvent.setup();
    const { hub } = await open('groups');
    await screen.findAllByText('everyone');
    const every = rowOf('everyone');
    await user.click(within(every).getByRole('button', { name: 'Edit' }));
    const dialog = screen.getByRole('dialog');
    await user.click(within(dialog).getByRole('combobox', { name: 'Search policies to add' }));
    await user.click(within(dialog).getByRole('option', { name: /read-only/ }));
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls).toContain('access.setGroupPolicies:21:30'));
  });

  it('adds a policy, and shows what the hub finds wrong with it', async () => {
    const user = userEvent.setup();
    const { hub } = await open('policies');
    expect(await screen.findByText('Built-in')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: '+ Add policy' }));
    const dialog = screen.getByRole('dialog');
    await user.type(within(dialog).getByLabelText(/^Name/), 'source-editor');
    hub.accessError = 'not valid HCL: line 3, column 1';
    await user.click(within(dialog).getByRole('button', { name: 'Add policy' }));
    expect(await within(dialog).findByText('not valid HCL: line 3, column 1')).toBeInTheDocument();
    hub.accessError = '';
    await user.click(within(dialog).getByRole('button', { name: 'Add policy' }));
    expect(await screen.findByText('source-editor')).toBeInTheDocument();
    expect(hub.calls).toContain('access.createPolicy:source-editor');
  });

  it('cannot delete a built-in policy but can edit its text', async () => {
    const user = userEvent.setup();
    const { hub } = await open('policies');
    const row = (await screen.findByText('read-only')).closest('tr')!;
    expect(within(row).queryByRole('button', { name: 'Delete' })).not.toBeInTheDocument();
    await user.click(within(row).getByRole('button', { name: 'View / edit' }));
    const dialog = screen.getByRole('dialog');
    expect(within(dialog).getByLabelText(/^Name/)).toBeDisabled();
    await user.type(within(dialog).getByLabelText('Policy (HCL)'), '# note');
    await user.click(within(dialog).getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(hub.calls.some((c) => c.startsWith('access.updatePolicy:30:{"hcl":'))).toBe(true));
  });

  it('asks the hub whether a user would be allowed, and shows why', async () => {
    const user = userEvent.setup();
    const { hub } = await open('policies');
    await screen.findByText('read-only');
    await user.type(screen.getByLabelText('Path'), 'sources/s1');
    await user.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText('allowed by read-only')).toBeInTheDocument();
    expect(hub.calls).toContain('access.check:1:read:sources/s1');
    hub.check = { allowed: false, reason: 'denied by freeze' };
    await user.click(screen.getByRole('button', { name: 'Test' }));
    expect(await screen.findByText('denied by freeze')).toBeInTheDocument();
  });
});

describe('what the person may see in the settings', () => {
  it('offers only the sections their rules let them read', async () => {
    const grants = [{ path: 'users', capabilities: ['read'] }, { path: 'settings', capabilities: ['read'] }];
    await renderWithHub(<Settings arg="policies" />, { session: { auth: { setupRequired: false, authenticated: true, publicView: false, username: 'dev', grants } } });
    expect(screen.getByRole('link', { name: 'Users' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Groups' })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Policies' })).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Profile' })).toBeInTheDocument();
    // a section they may not read falls back to General instead of showing refusals
    expect(screen.getByRole('heading', { name: 'TV display' })).toBeInTheDocument();
  });
});
