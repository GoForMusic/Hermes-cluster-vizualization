// The logged-in admin: change the password.
import { useState } from 'react';
import { useSession } from '../../../app/session';
import { useClient } from '../../../state/context';
import { useAsyncSubmit } from '../../../ui/hooks';
import { PageHead } from '../PageHead';

export function Account() {
  const client = useClient();
  const { auth } = useSession();
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [again, setAgain] = useState('');
  const [ok, setOk] = useState('');

  const { busy, error, submit } = useAsyncSubmit(
    async () => {
      await client.auth.changePassword(current, next);
      setOk('Password changed. Other sessions were signed out.');
      setCurrent(''); setNext(''); setAgain('');
    },
    () => { setOk(''); return next !== again ? 'The new passwords do not match.' : null; },
  );

  return (
    <>
      <PageHead title="Account" />
      <div className="card" style={{ maxWidth: 480 }}>
        <h3>Signed in as {auth.username}</h3>
        <form className="form" onSubmit={submit}>
          <label className="f">Current password<input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} /></label>
          <label className="f">New password (at least 10 characters)<input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} /></label>
          <label className="f">Repeat new password<input type="password" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} /></label>
          <div className={ok ? 'ok' : 'err'} role="status">{ok || error}</div>
          <button type="submit" className="btn primary" disabled={busy}>Change password</button>
        </form>
      </div>
    </>
  );
}
