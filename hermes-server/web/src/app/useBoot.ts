// Finds out who is looking, loads what they may see and starts following the hub: the steps that come before any screen.
import { useCallback, useEffect, useRef, useState } from 'react';
import type { AuthStatus } from '../generated/AuthStatus';
import { HubError } from '../hub/HubClient';
import { useHub } from '../state/context';
import { errorText as message } from '../ui/hooks';

export type Phase =
  | { kind: 'loading' }
  | { kind: 'setup' }
  | { kind: 'login' }
  | { kind: 'fatal'; message: string }
  | { kind: 'ready'; auth: AuthStatus };

/** Dispatched by the HTTP client when the hub says the login is needed. */
export const AUTH_REQUIRED = 'hermes:auth-required';

/** `admin`: the page being opened is the admin panel, which needs a login whatever the wallboard settings say. */
export function useBoot(admin: boolean): { phase: Phase; reboot: () => void; refreshAuth: () => Promise<void> } {
  const { client, store } = useHub();
  const [phase, setPhase] = useState<Phase>({ kind: 'loading' });
  const [attempt, setAttempt] = useState(0);
  const adminRef = useRef(admin);
  adminRef.current = admin;
  const reboot = useCallback(() => setAttempt((n) => n + 1), []);

  useEffect(() => {
    let cancelled = false;
    let stop: (() => void) | undefined;
    (async () => {
      setPhase({ kind: 'loading' });
      let auth: AuthStatus;
      try { auth = await client.auth.status(); } catch (e) { if (!cancelled) setPhase({ kind: 'fatal', message: message(e) }); return; }
      if (cancelled) return;
      if (auth.setupRequired) { setPhase({ kind: 'setup' }); return; }
      if (!auth.authenticated && (adminRef.current || !auth.publicView)) { setPhase({ kind: 'login' }); return; }
      try {
        await store.load(auth.authenticated); // source details are for admins only
      } catch (e) {
        if (cancelled) return;
        setPhase(e instanceof HubError && e.authRequired ? { kind: 'login' } : { kind: 'fatal', message: message(e) });
        return;
      }
      if (cancelled) return;
      stop = store.startLive(reboot); // the hub refused the stream: find out who this is again
      setPhase({ kind: 'ready', auth });
    })();
    return () => { cancelled = true; stop?.(); };
  }, [client, store, attempt, reboot]);

  useEffect(() => {
    window.addEventListener(AUTH_REQUIRED, reboot);
    return () => window.removeEventListener(AUTH_REQUIRED, reboot);
  }, [reboot]);

  // the admin panel was opened from a screen that needed no login: ask for it
  useEffect(() => { if (admin && phase.kind === 'ready' && !phase.auth.authenticated) reboot(); }, [admin, phase, reboot]);

  const refreshAuth = useCallback(async () => {
    const auth = await client.auth.status();
    setPhase((p) => (p.kind === 'ready' ? { kind: 'ready', auth } : p));
  }, [client]);

  return { phase, reboot, refreshAuth };
}
