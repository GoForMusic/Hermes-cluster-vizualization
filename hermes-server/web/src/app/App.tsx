// The composition of the screens: the boot steps decide what is shown, the address decides which screen.
import { useMemo, useState } from 'react';
import { AdminShell } from '../features/admin/AdminShell';
import { AuthScreen } from '../features/auth/AuthScreen';
import { SetupRegistry } from '../features/registry/SetupRegistry';
import { Landing } from '../features/landing/Landing';
import { TvScreen } from '../features/tv/TvScreen';
import { TvStatus } from '../features/tv/TvStatus';
import { useHub } from '../state/context';
import { useHashRoute } from '../ui/hooks';
import { SessionContext, type Session } from './session';
import { useBoot } from './useBoot';

export function App() {
  const { client } = useHub();
  const route = useHashRoute();
  const { phase, reboot, refreshAuth } = useBoot(route.view === 'admin');
  const [registryStep, setRegistryStep] = useState(false); // the account is made: the second step of the setup follows

  const session = useMemo<Session | null>(() => {
    if (phase.kind !== 'ready') return null;
    return { auth: phase.auth, refresh: refreshAuth, logout: () => client.auth.logout().finally(() => { location.hash = ''; location.reload(); }) };
  }, [phase, refreshAuth, client]);

  return (
    <>
      {phase.kind === 'setup' && !registryStep ? <AuthScreen mode="setup" onDone={() => setRegistryStep(true)} /> : null}
      {phase.kind === 'setup' && registryStep ? <SetupRegistry onDone={() => { setRegistryStep(false); reboot(); }} /> : null}
      {phase.kind === 'login' ? <AuthScreen mode="login" onDone={reboot} /> : null}
      {phase.kind === 'fatal' ? (
        <div className="fatal">
          <h1>Cannot reach the hub</h1>
          <p className="muted">{phase.message}</p>
          <button className="btn primary" onClick={reboot}>Retry</button>
        </div>
      ) : null}
      {session ? (
        <SessionContext.Provider value={session}>
          {route.view === 'tv' ? (route.page === 'status' ? <TvStatus /> : <TvScreen />) : route.view === 'admin' ? <AdminShell page={route.page || 'dashboard'} arg={route.arg} /> : <Landing />}
        </SessionContext.Provider>
      ) : null}
    </>
  );
}
