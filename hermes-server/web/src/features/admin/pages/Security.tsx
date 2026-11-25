// Settings → Security: how long a login lasts.
import { useEffect, useState } from 'react';
import { useClient } from '../../../state/context';
import { errorText, useAsyncSubmit } from '../../../ui/hooks';

export function Security() {
  const client = useClient();
  const [maxHours, setMaxHours] = useState('');
  const [idle, setIdle] = useState('');
  const [loadError, setLoadError] = useState('');
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    client.auth.sessionPolicy()
      .then((p) => { setMaxHours(String(p.maxHours)); setIdle(String(p.idleMinutes)); })
      .catch((e) => setLoadError(errorText(e)));
  }, [client]);
  const { busy, error, submit } = useAsyncSubmit(
    async () => {
      const p = await client.auth.setSessionPolicy({ maxHours: Number(maxHours), idleMinutes: Number(idle) });
      setMaxHours(String(p.maxHours)); setIdle(String(p.idleMinutes));
      setSaved(true);
    },
    () => {
      setSaved(false);
      const h = Number(maxHours), m = Number(idle);
      if (!Number.isInteger(h) || h < 1 || h > 720) return 'A login lasts between 1 and 720 hours (30 days).';
      if (!Number.isInteger(m) || !(m === 0 || (m >= 5 && m <= 1440))) return 'The inactivity limit is 0 (never) or between 5 and 1440 minutes.';
      return null;
    },
  );
  return (
    <>
      <h2 className="section-h">Login sessions</h2>
      <div className="card" style={{ maxWidth: 640 }}>
        <p className="help" style={{ marginTop: 0 }}>
          A new limit counts for the people who are already logged in too: the next thing each of them does is judged by it. Signing in again is all it costs them.
        </p>
        <form className="form" onSubmit={submit}>
          <label className="f">Log out after (hours)
            <input type="number" min={1} max={720} value={maxHours} onChange={(e) => setMaxHours(e.target.value)} />
            <span className="muted" style={{ fontSize: 11.5 }}>However busy someone is, they sign in again this long after they did. 336 is 14 days.</span>
          </label>
          <label className="f">Log out after being away (minutes)
            <input type="number" min={0} max={1440} value={idle} onChange={(e) => setIdle(e.target.value)} />
            <span className="muted" style={{ fontSize: 11.5 }}>0 turns it off. It counts what a person does, not an open page: a wallboard that is logged in (View without login is off) is signed out by it too, so give the wallboard a long limit or turn View without login on.</span>
          </label>
          <div className={saved ? 'ok' : 'err'} role="status">{saved ? 'Saved.' : error || loadError}</div>
          <div><button type="submit" className="btn primary" disabled={busy || !maxHours}>Save</button></div>
        </form>
      </div>
    </>
  );
}
