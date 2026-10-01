// Who is looking at the page, shared by the screens that need it (the admin shell, the account page, the TV display settings).
import { createContext, useContext } from 'react';
import type { AuthStatus } from '../generated/AuthStatus';

export interface Session {
  auth: AuthStatus;
  /** Ask the hub again who this is (after the wallboard was made public or private, for instance). */
  refresh: () => Promise<void>;
  logout: () => Promise<void>;
}

export const SessionContext = createContext<Session | null>(null);

export function useSession(): Session {
  const s = useContext(SessionContext);
  if (!s) throw new Error('useSession must be used inside the session provider');
  return s;
}
