// The pages of the admin panel, as data: adding a page is adding a line here. The shell knows nothing about any of them.
import type { ComponentType } from 'react';
import { can, canSeeClusters } from '../../domain/access';
import type { Grant } from '../../generated/Grant';
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
  /** Is the page for this person? Nothing: always. What a person is not allowed to read is not offered: the hub would only answer with refusals. */
  allowed?: (grants: readonly Grant[] | undefined) => boolean;
}

export const ADMIN_PAGES: readonly AdminPage[] = [
  { id: 'dashboard', label: 'Dashboard', Component: Dashboard, allowed: canSeeClusters },
  { id: 'topology', label: 'Topology', Component: Topology, allowed: canSeeClusters },
  { id: 'sources', label: 'Sources', Component: Sources, allowed: (g) => can(g, 'read', 'sources') },
  { id: 'rules', label: 'Alert rules', Component: Rules, allowed: (g) => can(g, 'read', 'settings') },
  { id: 'alerts', label: 'Alerts', Component: Alerts, allowed: canSeeClusters },
  { id: 'logs', label: 'Logs', Component: Logs, allowed: (g) => can(g, 'read', 'comm-log') },
  // always: at least the person's own profile is in it
  { id: 'settings', label: 'Settings', Component: Settings },
];
