import { useEffect, useRef, useState } from 'react';
import type { HubState } from '../../domain/hubState';
import { useHubState, useStore } from '../../state/context';
import { beep } from '../../ui/audio';
import { useNow } from '../../ui/hooks';

export interface FeedEntry {
  id: number;
  cls: 'ok' | 'warn' | 'crit';
  text: string;
  sub: string;
  until: number;
}

const SHOWN = 5;
const LIFETIME_MS = 10_000;
const alertsOf = (s: HubState) => s.alerts;

/** The entries of the kill feed: a new alert, and "X recovered" when one resolves. They fade after ten seconds. */
export function useKillFeed(sound: boolean): FeedEntry[] {
  const store = useStore();
  const alerts = useHubState(alertsOf);
  const [entries, setEntries] = useState<FeedEntry[]>([]);
  const next = useRef(1);
  const seenResolved = useRef(new Set(alerts.filter((a) => a.resolvedTs != null).map((a) => a.id)));
  const soundOn = useRef(sound);
  soundOn.current = sound;
  const now = useNow(1000);

  const add = (cls: FeedEntry['cls'], text: string, sub: string) =>
    setEntries((list) => [{ id: next.current++, cls, text, sub, until: Date.now() + LIFETIME_MS }, ...list].slice(0, SHOWN));

  useEffect(() => store.onNewAlert((a) => {
    add(a.sev === 'crit' ? 'crit' : 'warn', a.title, a.sev === 'crit' ? '' : a.detail);
    if (a.sev === 'crit' && soundOn.current) beep();
  }), [store]);

  useEffect(() => {
    for (const a of alerts) {
      if (a.resolvedTs != null && !seenResolved.current.has(a.id)) {
        seenResolved.current.add(a.id);
        add('ok', `${a.title.replace(/ (unreachable|CrashLoopBackOff|failing)$/i, '')} recovered`, '');
      }
    }
  }, [alerts]);

  return entries.filter((e) => e.until > now);
}
