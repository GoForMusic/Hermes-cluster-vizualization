// What the hub knows about its datacenters (or about being a member), for the screens that offer a choice of hub.
import { useEffect, useState } from 'react';
import type { FederationView } from '../../generated/FederationView';
import { useClient } from '../../state/context';

const NONE: FederationView = { name: '', member: null, datacenters: [] };

/** Loads the federation when `active` turns true, and again every `everyMs` while it is (0: only once). A hub that cannot say gives none. */
export function useFederation(active = true, everyMs = 0): { view: FederationView; reload: () => Promise<void> } {
  const client = useClient();
  const [view, setView] = useState<FederationView>(NONE);
  const reload = async (): Promise<void> => {
    try { setView(await client.federation.view()); } catch { /* not an admin, or the hub is unreachable: no datacenters to offer */ }
  };
  useEffect(() => {
    if (!active) return;
    let live = true;
    const load = async () => { try { const v = await client.federation.view(); if (live) setView(v); } catch { /* see above */ } };
    void load();
    const timer = everyMs ? setInterval(() => void load(), everyMs) : undefined;
    return () => { live = false; if (timer) clearInterval(timer); };
  }, [client, active, everyMs]);
  return { view, reload };
}
