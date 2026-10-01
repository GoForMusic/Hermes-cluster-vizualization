// The pages of the admin panel, as data: adding a page is adding a line here. The shell knows nothing about any of them.
import type { ComponentType } from 'react';
import { Account } from './pages/Account';
import { Alerts } from './pages/Alerts';
import { Dashboard } from './pages/Dashboard';
import { Logs } from './pages/Logs';
import { Rules } from './pages/Rules';
import { Settings } from './pages/Settings';
import { Sources } from './pages/Sources';
import { Topology } from './pages/Topology';

export interface AdminPage {
  id: string;
  label: string;
  Component: ComponentType<{ arg: string }>;
}

export const ADMIN_PAGES: readonly AdminPage[] = [
  { id: 'dashboard', label: 'Dashboard', Component: Dashboard },
  { id: 'topology', label: 'Topology', Component: Topology },
  { id: 'sources', label: 'Sources', Component: Sources },
  { id: 'rules', label: 'Alert rules', Component: Rules },
  { id: 'alerts', label: 'Alerts', Component: Alerts },
  { id: 'logs', label: 'Logs', Component: Logs },
  { id: 'settings', label: 'Settings', Component: Settings },
  { id: 'account', label: 'Account', Component: Account },
];
