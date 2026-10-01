// First-run setup and login (the same idea as Uptime Kuma: the first visit creates the admin).
import { useState, type ReactNode } from 'react';
import { useClient } from '../../state/context';
import { Brand } from '../../ui/brand';
import { useAsyncSubmit } from '../../ui/hooks';

const Field = ({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) => (
  <label className="f">{label}{children}{hint ? <span className="muted" style={{ fontSize: 11.5 }}>{hint}</span> : null}</label>
);

export function AuthScreen({ mode, onDone }: { mode: 'setup' | 'login'; onDone: () => void }) {
  const client = useClient();
  const setup = mode === 'setup';
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [publicView, setPublicView] = useState(true);

  const { busy, error, submit } = useAsyncSubmit(
    async () => {
      if (setup) await client.auth.setup({ username, password, publicView });
      else await client.auth.login({ username, password });
      onDone();
    },
    () => (setup && password !== confirm ? 'The two passwords do not match.' : null),
  );

  return (
    <div className="auth-wrap">
      <form className="card auth-card" onSubmit={submit}>
        <div className="auth-brand"><Brand compact /></div>
        <h1>{setup ? 'Create the admin account' : 'Log in'}</h1>
        {setup ? <p className="help">First start: choose the account that manages sources, alert rules and settings. Only the password hash is stored.</p> : null}
        <div className="form">
          <Field label="Username" hint={setup ? '3–32 characters: letters, digits, . - _' : undefined}>
            <input type="text" autoComplete="username" placeholder="admin" autoFocus value={username} onChange={(e) => setUsername(e.target.value)} />
          </Field>
          <Field label="Password" hint={setup ? 'At least 10 characters' : undefined}>
            <input type="password" autoComplete={setup ? 'new-password' : 'current-password'} value={password} onChange={(e) => setPassword(e.target.value)} />
          </Field>
          {setup ? <Field label="Repeat password"><input type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} /></Field> : null}
          {setup ? (
            <label className="check">
              <input type="checkbox" checked={publicView} onChange={(e) => setPublicView(e.target.checked)} />
              <span>
                <b>Allow the wallboard (TV) without login</b>
                <span className="muted">Lets anyone who can reach this hub see the read-only view. Fine on a trusted LAN; turn it off if the hub is reachable by people you do not trust. You can change it later.</span>
              </span>
            </label>
          ) : null}
          <div className="err" role="alert">{error}</div>
          <button className="btn primary" type="submit" disabled={busy}>{setup ? 'Create admin' : 'Log in'}</button>
        </div>
      </form>
    </div>
  );
}
