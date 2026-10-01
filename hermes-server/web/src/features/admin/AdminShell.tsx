// The admin panel's frame: the top tabs, the page area and the status bar. The pages are in `adminPages.ts`.
import { useMemo, useState } from 'react';
import { useSession } from '../../app/session';
import { dtg } from '../../domain/format';
import { activeAlerts } from '../../domain/selectors';
import { shallowEqual, useHubState } from '../../state/context';
import { useNow } from '../../ui/hooks';
import { Brand } from '../../ui/brand';
import { StatusTags } from '../shared/StatusTags';
import { AdminShellContext } from './adminContext';
import { ADMIN_PAGES } from './adminPages';

export function AdminShell({ page, arg }: { page: string; arg: string }) {
  const { auth, logout } = useSession();
  const alerts = useHubState((s) => activeAlerts(s), shallowEqual);
  const [cursor, setCursor] = useState('GRID —');
  const now = useNow(1000);
  const api = useMemo(() => ({ setCursor }), []);
  const current = ADMIN_PAGES.find((p) => p.id === page) ?? ADMIN_PAGES[0]!;
  const Page = current.Component;
  const anyCrit = alerts.some((a) => a.sev === 'crit');

  return (
    <div className="admin">
      <header className="ribbon">
        <div className="brand"><Brand /></div>
        <nav>
          {ADMIN_PAGES.map((p) => (
            <a key={p.id} className={`nav${p.id === current.id ? ' on' : ''}`} href={`#/admin/${p.id}`}>
              {p.label}
              {p.id === 'alerts' && alerts.length ? <span className={`badge${anyCrit ? '' : ' warn'}`}>{alerts.length}</span> : null}
            </a>
          ))}
        </nav>
        <div className="spacer" />
        <a className="tvlink" href="#/tv">TV view ↗</a>
        <button className="btn" title={`Signed in as ${auth.username}`} onClick={() => void logout()}>Log out ({auth.username})</button>
      </header>
      <AdminShellContext.Provider value={api}>
        <main className="content"><Page key={current.id} arg={current.id === page ? arg : ''} /></main>
      </AdminShellContext.Provider>
      <footer className="statusbar">
        <StatusTags />
        <span id="sb-cursor">{cursor}</span>
        <span className="spacer" />
        <span>Operator: {auth.username}</span>
        <span className="hud">DTG {dtg(new Date(now))}</span>
      </footer>
    </div>
  );
}
