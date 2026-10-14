// The two screens of a hub that another hub manages: the pairing code it shows while it waits for the master, and the lock once the master came.
import { useEffect, useState } from 'react';
import type { MemberView } from '../../generated/MemberView';
import type { MemberCode } from '../../hub/HubClient';
import { useClient } from '../../state/context';
import { Brand } from '../../ui/brand';
import { errorText, useNow } from '../../ui/hooks';

/** How long until `at` (ms since the epoch), as "42 min" or "9 min". */
const minutesLeft = (at: number, now: number): number => Math.max(0, Math.ceil((at - now) / 60_000));

/** Shows the code of a hub that waits for its master. `first`: the code just made, which is only ever shown once. */
export function PairingScreen({ member, first, onPaired, onCancel }: { member: MemberView; first: MemberCode | null; onPaired: () => void; onCancel: () => void }) {
  const client = useClient();
  const now = useNow(5000);
  const [code, setCode] = useState<{ code: string; expiresAt: number } | null>(first);
  const [error, setError] = useState('');

  // the master came: the hub is locked now
  useEffect(() => {
    const t = setInterval(() => { void client.auth.status().then((s) => { if (s.member?.state === 'paired') onPaired(); }).catch(() => {}); }, 3000);
    return () => clearInterval(t);
  }, [client, onPaired]);

  const renew = async () => {
    setError('');
    try { const c = await client.federation.newCode(); setCode(c); } catch (e) { setError(errorText(e)); }
  };
  const cancel = async () => {
    try { await client.federation.disableMember(); onCancel(); } catch (e) { setError(errorText(e)); }
  };
  const expiresAt = code?.expiresAt ?? member.codeExpires ?? 0;
  const expired = expiresAt > 0 && expiresAt <= now;

  return (
    <div className="auth-wrap">
      <div className="card auth-card">
        <div className="auth-brand"><Brand compact /></div>
        <h1>Waiting for its master</h1>
        <p className="help">
          This hub will be managed by another hub. In the parent hub open <b>Settings → Federation → Add datacenter</b> and give it this address and this code. The other hub connects to
          this one (nothing here needs to reach out).
        </p>
        <div className="pair-box">
          <span className="muted">Pairing code</span>
          {code && !expired ? <code className="pair-code" data-testid="pair-code">{code.code}</code> : <span className="muted">{expired ? 'The code expired.' : 'The code is shown only once, when it is made.'}</span>}
          {code && !expired ? <span className="muted">works for {minutesLeft(expiresAt, now)} min, once</span> : null}
        </div>
        <div className="pair-box">
          <span className="muted">Address of this hub</span>
          <code className="pair-code small">{member.agentAddress || location.origin}</code>
          <span className="muted">also what the agents of this datacenter connect to</span>
        </div>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions" style={{ justifyContent: 'flex-start' }}>
          <button type="button" className="btn primary" onClick={() => void renew()}>{code && !expired ? 'New code' : 'Show a new code'}</button>
          <button type="button" className="btn" onClick={() => void cancel()}>Use this hub on its own</button>
        </div>
      </div>
    </div>
  );
}

/** A hub with a master: no login, nothing to do here. */
export function ManagedScreen({ member }: { member: MemberView }) {
  return (
    <div className="auth-wrap">
      <div className="card auth-card">
        <div className="auth-brand"><Brand compact /></div>
        <h1>Managed by {member.managedBy || 'another hub'}</h1>
        {member.datacenter ? <p className="muted" style={{ marginTop: -4 }}>This datacenter is called <b>{member.datacenter}</b> there.</p> : null}
        <p className="help">
          This hub has no login of its own: its sources, registry and settings are controlled from the hub that manages it, and what it sees is shown there. The agents of this datacenter keep
          reporting here.
        </p>
        <p className="help" style={{ marginBottom: 0 }}>
          To make it a hub on its own again, remove it in that hub (Settings → Federation). If that hub is gone for good, restart this one once with <code>HUB_FEDERATION=off</code> and then remove the variable.
        </p>
      </div>
    </div>
  );
}
