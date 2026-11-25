// Settings → Policies: what a group may do, written the way Vault writes it (HCL). A policy is given to groups (Settings → Groups).
import { useCallback, useState } from 'react';
import type { PolicyView } from '../../../generated/PolicyView';
import type { AccessCheck } from '../../../generated/AccessCheck';
import { useClient } from '../../../state/context';
import { Dialog } from '../../../ui/dialog';
import { Icon } from '../../../ui/icons';
import { ListBar, usePaged } from '../../../ui/paging';
import { StatusChip } from '../../../ui/status';
import { errorText, useAsyncSubmit } from '../../../ui/hooks';
import { useAccess } from './Access';

const EXAMPLE = `# What the groups this policy is given to may do.
# The path is the URL without /api/: GET /api/sources/s1 is "read" on "sources/s1".
# "*" at the end means everything below, "+" means exactly one segment.
path "sources/*" {
  capabilities = ["read", "list"]
}

# deny wins over every other rule
path "sources/prod-*" {
  capabilities = ["deny"]
}
`;

export function PoliciesTab() {
  const { users, groups, policies, error, reload } = useAccess();
  const [editing, setEditing] = useState<PolicyView | 'new' | null>(null);
  const [removing, setRemoving] = useState<PolicyView | null>(null);
  const names = new Map(groups.map((g) => [g.id, g.name]));
  const matches = useCallback((p: PolicyView, text: string) => `${p.name} ${p.description} ${p.groups.map((g) => names.get(g) ?? '').join(' ')}`.toLowerCase().includes(text), [names]);
  const paged = usePaged(policies, matches);
  return (
    <>
      {error ? <div className="err" role="alert">{error}</div> : null}
      <p className="help">
        A policy is a list of <code>path</code> rules, each with the <code>capabilities</code> it gives: <code>read</code>, <code>list</code>, <code>create</code>, <code>update</code>,
        <code> delete</code> or <code>deny</code>. Every rule that matches counts, and <code>deny</code> beats the rest. A policy does nothing until a group is given it.
      </p>
      <div className="card">
        <div className="card-head"><h3>Policies</h3><button className="btn primary xs" onClick={() => setEditing('new')}>+ Add policy</button></div>
        <ListBar paged={paged} label="Search policies" placeholder="Search by name or group…" />
        {paged.total ? (
          <table className="tbl">
            <thead><tr>{['Policy', 'Given to', ''].map((t) => <th key={t}>{t}</th>)}</tr></thead>
            <tbody>
              {paged.shown.map((p) => (
                <tr key={p.id}>
                  <td>
                    <span className="who"><Icon name="policy" size={16} /><b>{p.name}</b></span>{' '}
                    {p.builtin ? <StatusChip status="unknown">Built-in</StatusChip> : null}
                    {p.description ? <div className="muted">{p.description}</div> : null}
                  </td>
                  <td>{p.groups.length ? p.groups.map((g) => names.get(g) ?? '?').join(', ') : <span className="muted">no group</span>}</td>
                  <td className="r">
                    <button className="btn xs" onClick={() => setEditing(p)}>{p.builtin ? 'View / edit' : 'Edit'}</button>{' '}
                    {p.builtin ? null : <button className="btn xs danger" onClick={() => setRemoving(p)}>Delete</button>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <div className="empty"><b>{policies.length ? 'No policy matches' : 'No policies yet'}</b>{policies.length ? 'Try another search.' : 'Add one, then give it to a group.'}</div>}
      </div>
      <TestAccess users={users.map((u) => ({ id: u.id, username: u.username }))} />
      {editing ? <EditPolicy key={editing === 'new' ? 'new' : editing.id} policy={editing === 'new' ? null : editing} onClose={() => setEditing(null)} onDone={reload} /> : null}
      {removing ? <DeletePolicy key={removing.id} policy={removing} onClose={() => setRemoving(null)} onDone={reload} /> : null}
    </>
  );
}

function EditPolicy({ policy, onClose, onDone }: { policy: PolicyView | null; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const [name, setName] = useState(policy?.name ?? '');
  const [description, setDescription] = useState(policy?.description ?? '');
  const [hcl, setHcl] = useState(policy?.hcl ?? EXAMPLE);
  const { busy, error, submit } = useAsyncSubmit(async () => {
    if (policy) {
      const patch: { name?: string; description?: string; hcl?: string } = {};
      if (name !== policy.name) patch.name = name;
      if (description !== policy.description) patch.description = description;
      if (hcl !== policy.hcl) patch.hcl = hcl;
      if (Object.keys(patch).length) await client.access.updatePolicy(policy.id, patch);
    } else {
      await client.access.createPolicy({ name, description, hcl });
    }
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose} wide>
      <h2>{policy ? policy.name : 'Add policy'}</h2>
      <form className="form" onSubmit={submit}>
        <label className="f">Name (lowercase letters, digits, dot, dash, underscore)<input value={name} disabled={policy?.builtin} onChange={(e) => setName(e.target.value)} autoComplete="off" /></label>
        <label className="f">Description (optional)<input value={description} onChange={(e) => setDescription(e.target.value)} autoComplete="off" /></label>
        <label className="f">Policy (HCL)<textarea className="hcl" spellCheck={false} value={hcl} onChange={(e) => setHcl(e.target.value)} /></label>
        <div className="err" role="alert" style={{ whiteSpace: 'pre-wrap' }}>{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>{policy ? 'Save' : 'Add policy'}</button>
        </div>
      </form>
    </Dialog>
  );
}

function DeletePolicy({ policy, onClose, onDone }: { policy: PolicyView; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await client.access.deletePolicy(policy.id);
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose}>
      <h2>Delete {policy.name}?</h2>
      <p className="help" style={{ marginTop: 0 }}>
        {policy.groups.length ? `The groups it is given to (${policy.groups.length}) stop giving it: their users lose what it allowed. ` : ''}This cannot be undone.
      </p>
      <form onSubmit={submit}>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn danger solid" disabled={busy}>Delete policy</button>
        </div>
      </form>
    </Dialog>
  );
}

/** Asks the hub, with the real policies, what a user would be allowed: the way to check a policy before relying on it. */
function TestAccess({ users }: { users: { id: number; username: string }[] }) {
  const client = useClient();
  const [user, setUser] = useState('');
  const [capability, setCapability] = useState('read');
  const [path, setPath] = useState('');
  const [answer, setAnswer] = useState<AccessCheck | null>(null);
  const [error, setError] = useState('');
  const ask = (e: React.FormEvent) => {
    e.preventDefault();
    setAnswer(null); setError('');
    client.access.check(Number(user || users[0]?.id), capability, path).then(setAnswer).catch((ex) => setError(errorText(ex)));
  };
  return (
    <div className="card" style={{ marginTop: 16 }}>
      <div className="card-head"><h3>Test access</h3></div>
      <p className="help" style={{ marginTop: 0 }}>Would this user be allowed? It asks the hub, with the policies as they are saved now. The path is the URL without /api/, like <code>sources/s1</code>.</p>
      <form className="form" onSubmit={ask} style={{ display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'flex-end' }}>
        <label className="f">User
          <select value={user || String(users[0]?.id ?? '')} onChange={(e) => setUser(e.target.value)}>
            {users.map((u) => <option key={u.id} value={u.id}>{u.username}</option>)}
          </select>
        </label>
        <label className="f">Capability
          <select value={capability} onChange={(e) => setCapability(e.target.value)}>
            {['read', 'create', 'update', 'delete'].map((c) => <option key={c}>{c}</option>)}
          </select>
        </label>
        <label className="f" style={{ flex: '1 1 220px' }}>Path<input value={path} onChange={(e) => setPath(e.target.value)} placeholder="sources/s1" autoComplete="off" /></label>
        <button type="submit" className="btn primary" disabled={!path.trim() || !users.length}>Test</button>
      </form>
      {answer ? <p role="status" style={{ marginBottom: 0 }}><StatusChip status={answer.allowed ? 'ok' : 'crit'}>{answer.allowed ? 'Allowed' : 'Refused'}</StatusChip> {answer.reason}</p> : null}
      {error ? <div className="err" role="alert">{error}</div> : null}
    </div>
  );
}
