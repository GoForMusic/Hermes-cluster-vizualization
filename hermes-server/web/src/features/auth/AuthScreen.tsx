// First-run setup and login (the same idea as Uptime Kuma: the first visit creates the admin).
import { useState, type ReactNode } from 'react';
import type { MemberCode } from '../../hub/HubClient';
import { useClient } from '../../state/context';
import { Brand } from '../../ui/brand';
import { useAsyncSubmit } from '../../ui/hooks';

const Field = ({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) => (
  <label className="f">{label}{children}{hint ? <span className="muted" style={{ fontSize: 11.5 }}>{hint}</span> : null}</label>
);

export function AuthScreen({ mode, onDone, onMember }: { mode: 'setup' | 'login'; onDone: () => void; onMember?: (code: MemberCode) => void }) {
  const client = useClient();
  const setup = mode === 'setup';
  const [managed, setManaged] = useState(false); // first run: this hub will be managed by another one, so it gets no admin of its own
  const [address, setAddress] = useState(location.origin);
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

  const member = useAsyncSubmit(async () => { onMember?.(await client.federation.enableMember(address.trim())); });

  if (setup && managed) {
    return (
      <div className="auth-wrap">
        <form className="card auth-card" onSubmit={member.submit}>
          <div className="auth-brand"><Brand compact /></div>
          <SetupKind managed onChange={setManaged} />
          <h1>Managed by another hub</h1>
          <p className="help">
            Use this when the hub that watches your clusters is in another network zone and this one sits next to the machines it cannot reach. This hub then shows a pairing code and
            the other hub connects to it (so the firewall only has to allow that direction). Afterwards there is no login here: everything is done from the other hub.
          </p>
          <div className="form">
            <Field label="Address of this hub" hint="How the other hub and the agents next to this one reach it (this page's address, as they see it). Not localhost.">
              <input type="text" value={address} onChange={(e) => setAddress(e.target.value)} />
            </Field>
            <div className="err" role="alert">{member.error}</div>
            <button className="btn primary" type="submit" disabled={member.busy || !address.trim()}>Show the pairing code</button>
          </div>
        </form>
      </div>
    );
  }

  return (
    <div className="auth-wrap">
      <form className="card auth-card" onSubmit={submit}>
        <div className="auth-brand"><Brand compact /></div>
        {setup && onMember ? <SetupKind managed={false} onChange={setManaged} /> : null}
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

/** First run: a hub of its own (with an admin), or one that another hub manages. */
function SetupKind({ managed, onChange }: { managed: boolean; onChange: (managed: boolean) => void }) {
  return (
    <div className="tabs" role="group" aria-label="What this hub is" style={{ marginBottom: 12 }}>
      <button type="button" className={`pill${managed ? '' : ' on'}`} onClick={() => onChange(false)}>A hub of its own</button>
      <button type="button" className={`pill${managed ? ' on' : ''}`} onClick={() => onChange(true)}>Managed by another hub</button>
    </div>
  );
}
