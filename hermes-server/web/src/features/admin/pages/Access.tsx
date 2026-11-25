// Settings → Users and Groups: who can log in, and the groups they are in. A break-glass account is a local account with a password that is never disabled or
// deleted: the way in when something else (an identity provider, later) is down or set up wrong.
import { useCallback, useEffect, useState } from 'react';
import type { GroupView } from '../../../generated/GroupView';
import type { UserView } from '../../../generated/UserView';
import { useClient } from '../../../state/context';
import { Dialog } from '../../../ui/dialog';
import { StatusChip } from '../../../ui/status';
import { errorText, useAsyncSubmit } from '../../../ui/hooks';
import { useSession } from '../../../app/session';

/** Both lists, loaded together: a user shows its groups and a group its members. */
function useAccess() {
  const client = useClient();
  const [users, setUsers] = useState<UserView[]>([]);
  const [groups, setGroups] = useState<GroupView[]>([]);
  const [error, setError] = useState('');
  const reload = useCallback(async () => {
    try {
      const [u, g] = await Promise.all([client.access.users(), client.access.groups()]);
      setUsers(u); setGroups(g); setError('');
    } catch (e) { setError(errorText(e)); }
  }, [client]);
  useEffect(() => { void reload(); }, [reload]);
  return { users, groups, error, reload };
}

export function UsersTab() {
  const { users, groups, error, reload } = useAccess();
  return <>{error ? <div className="err" role="alert">{error}</div> : null}<Users users={users} groups={groups} reload={reload} /></>;
}

export function GroupsTab() {
  const { users, groups, error, reload } = useAccess();
  return <>{error ? <div className="err" role="alert">{error}</div> : null}<Groups users={users} groups={groups} reload={reload} /></>;
}

interface Lists { users: UserView[]; groups: GroupView[]; reload: () => Promise<void> }

function Users({ users, groups, reload }: Lists) {
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<UserView | null>(null);
  const [removing, setRemoving] = useState<UserView | null>(null);
  const names = new Map(groups.map((g) => [g.id, g.name]));
  return (
    <>
      <p className="help">
        Everyone listed here can log in and, until policies arrive, does everything an admin does. A <b>break-glass</b> account is a local account that always
        keeps its password: it cannot be disabled or deleted, and there is always at least one, so a broken sign-in setup never locks everybody out.
      </p>
      <div className="card">
        <div className="card-head"><h3>Users</h3><button className="btn primary xs" onClick={() => setCreating(true)}>+ Add user</button></div>
        <table className="tbl">
          <thead><tr>{['User', 'Email', 'Groups', ''].map((t) => <th key={t}>{t}</th>)}</tr></thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.id}>
                <td>
                  <b>{u.username}</b>{' '}
                  {u.breakGlass ? <StatusChip status="warn">Break-glass</StatusChip> : null}{' '}
                  {u.disabled ? <StatusChip status="crit">Disabled</StatusChip> : null}
                </td>
                <td>{u.email || <span className="muted">—</span>}</td>
                <td>{u.groups.length ? u.groups.map((g) => names.get(g) ?? '?').join(', ') : <span className="muted">—</span>}</td>
                <td className="r">
                  <button className="btn xs" onClick={() => setEditing(u)}>Edit</button>{' '}
                  <button className="btn xs danger" onClick={() => setRemoving(u)}>Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <CreateUser open={creating} onClose={() => setCreating(false)} onDone={reload} />
      {editing ? <EditUser key={editing.id} user={editing} onClose={() => setEditing(null)} onDone={reload} /> : null}
      {removing ? <DeleteUser key={removing.id} user={removing} onClose={() => setRemoving(null)} onDone={reload} /> : null}
    </>
  );
}

function CreateUser({ open, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const [username, setUsername] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [breakGlass, setBreakGlass] = useState(false);
  useEffect(() => { if (open) { setUsername(''); setEmail(''); setPassword(''); setBreakGlass(false); } }, [open]);
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await client.access.createUser({ username, password, email, breakGlass });
    await onDone();
    onClose();
  });
  return (
    <Dialog open={open} onClose={onClose}>
      <h2>Add user</h2>
      <form className="form" onSubmit={submit}>
        <label className="f">Username<input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" /></label>
        <label className="f">Email (optional)<input type="email" value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="off" /></label>
        <label className="f">Password (at least 10 characters)<input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" /></label>
        <label className="f check"><input type="checkbox" checked={breakGlass} onChange={(e) => setBreakGlass(e.target.checked)} /> Break-glass: a local account that is never disabled or deleted</label>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>Add user</button>
        </div>
      </form>
    </Dialog>
  );
}

function EditUser({ user, onClose, onDone }: { user: UserView; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const { auth } = useSession();
  const me = auth.username === user.username;
  const [email, setEmail] = useState(user.email);
  const [breakGlass, setBreakGlass] = useState(user.breakGlass);
  const [disabled, setDisabled] = useState(user.disabled);
  const [password, setPassword] = useState('');
  const { busy, error, submit } = useAsyncSubmit(async () => {
    const patch: { email?: string; breakGlass?: boolean; disabled?: boolean; password?: string } = {};
    if (email !== user.email) patch.email = email;
    if (breakGlass !== user.breakGlass) patch.breakGlass = breakGlass;
    if (disabled !== user.disabled) patch.disabled = disabled;
    if (password) patch.password = password;
    if (Object.keys(patch).length) await client.access.updateUser(user.id, patch);
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose}>
      <h2>{user.username}</h2>
      <form className="form" onSubmit={submit}>
        <label className="f">Email<input type="email" value={email} onChange={(e) => setEmail(e.target.value)} autoComplete="off" /></label>
        <label className="f check"><input type="checkbox" checked={breakGlass} onChange={(e) => setBreakGlass(e.target.checked)} /> Break-glass account</label>
        <label className="f check"><input type="checkbox" checked={disabled} disabled={me} onChange={(e) => setDisabled(e.target.checked)} /> Disabled: cannot log in, and is logged out now</label>
        {me
          ? <p className="help">This is you: change your password in Profile.</p>
          : <label className="f">Set a new password (leave empty to keep it; their sessions end)<input type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="new-password" /></label>}
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>Save</button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteUser({ user, onClose, onDone }: { user: UserView; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await client.access.deleteUser(user.id);
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose}>
      <h2>Delete {user.username}?</h2>
      <p className="help" style={{ marginTop: 0 }}>The account is gone, it is logged out, and it leaves its groups. What it acknowledged in Alerts keeps its name. This cannot be undone.</p>
      <form onSubmit={submit}>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn danger solid" disabled={busy}>Delete user</button>
        </div>
      </form>
    </Dialog>
  );
}

function Groups({ users, groups, reload }: Lists) {
  const [editing, setEditing] = useState<GroupView | 'new' | null>(null);
  const [removing, setRemoving] = useState<GroupView | null>(null);
  const names = new Map(users.map((u) => [u.id, u.username]));
  return (
    <>
      <p className="help">A group is a set of users. Permissions are given to groups, not to people: that comes with policies.</p>
      <div className="card">
        <div className="card-head"><h3>Groups</h3><button className="btn primary xs" onClick={() => setEditing('new')}>+ Add group</button></div>
        {groups.length ? (
          <table className="tbl">
            <thead><tr>{['Group', 'Members', ''].map((t) => <th key={t}>{t}</th>)}</tr></thead>
            <tbody>
              {groups.map((g) => (
                <tr key={g.id}>
                  <td><b>{g.name}</b>{g.description ? <div className="muted">{g.description}</div> : null}</td>
                  <td>{g.members.length ? g.members.map((m) => names.get(m) ?? '?').join(', ') : <span className="muted">nobody</span>}</td>
                  <td className="r">
                    <button className="btn xs" onClick={() => setEditing(g)}>Edit</button>{' '}
                    <button className="btn xs danger" onClick={() => setRemoving(g)}>Delete</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <div className="empty"><b>No groups yet</b>Add one, then put users in it.</div>}
      </div>
      {editing ? <EditGroup key={editing === 'new' ? 'new' : editing.id} group={editing === 'new' ? null : editing} users={users} onClose={() => setEditing(null)} onDone={reload} /> : null}
      {removing ? <DeleteGroup key={removing.id} group={removing} onClose={() => setRemoving(null)} onDone={reload} /> : null}
    </>
  );
}

function EditGroup({ group, users, onClose, onDone }: { group: GroupView | null; users: UserView[]; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const [name, setName] = useState(group?.name ?? '');
  const [description, setDescription] = useState(group?.description ?? '');
  const [members, setMembers] = useState<number[]>(group?.members ?? []);
  const { busy, error, submit } = useAsyncSubmit(async () => {
    let id = group?.id;
    if (group) {
      const patch: { name?: string; description?: string } = {};
      if (name !== group.name) patch.name = name;
      if (description !== group.description) patch.description = description;
      if (Object.keys(patch).length) await client.access.updateGroup(group.id, patch);
    } else {
      id = (await client.access.createGroup({ name, description })).id;
    }
    if (id !== undefined && (!group || members.slice().sort().join() !== group.members.slice().sort().join())) await client.access.setMembers(id, members);
    await onDone();
    onClose();
  });
  const toggle = (id: number, on: boolean) => setMembers(on ? [...members, id] : members.filter((m) => m !== id));
  return (
    <Dialog open onClose={onClose}>
      <h2>{group ? group.name : 'Add group'}</h2>
      <form className="form" onSubmit={submit}>
        <label className="f">Name<input value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" /></label>
        <label className="f">Description (optional)<input value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" /></label>
        <fieldset className="f">
          <legend>Members</legend>
          {users.map((u) => (
            <label key={u.id} className="check"><input type="checkbox" checked={members.includes(u.id)} onChange={(e) => toggle(u.id, e.target.checked)} /> {u.username}</label>
          ))}
        </fieldset>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>{group ? 'Save' : 'Add group'}</button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteGroup({ group, onClose, onDone }: { group: GroupView; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await client.access.deleteGroup(group.id);
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose}>
      <h2>Delete {group.name}?</h2>
      <p className="help" style={{ marginTop: 0 }}>The users in it stay; they only leave the group.</p>
      <form onSubmit={submit}>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn danger solid" disabled={busy}>Delete group</button>
        </div>
      </form>
    </Dialog>
  );
}
