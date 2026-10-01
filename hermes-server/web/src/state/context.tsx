// How the components reach the store and the client: through React context, so they never import a concrete implementation.
import { createContext, useCallback, useContext, useRef, useSyncExternalStore, type ReactNode } from 'react';
import type { HubState } from '../domain/hubState';
import type { IHubClient } from '../hub/HubClient';
import type { HubStore } from './HubStore';

interface Hub {
  client: IHubClient;
  store: HubStore;
}

const HubContext = createContext<Hub | null>(null);

export function HubProvider({ client, store, children }: Hub & { children: ReactNode }) {
  return <HubContext.Provider value={{ client, store }}>{children}</HubContext.Provider>;
}

export function useHub(): Hub {
  const hub = useContext(HubContext);
  if (!hub) throw new Error('useHub must be used inside a HubProvider');
  return hub;
}

export const useStore = (): HubStore => useHub().store;
export const useClient = (): IHubClient => useHub().client;

export const shallowEqual = <T,>(a: readonly T[], b: readonly T[]): boolean => a.length === b.length && a.every((x, i) => x === b[i]);

/**
 * Reads a part of the state. The component renders again only when what the selector returns changes (by `equals`), not on every event:
 * a metrics sample every second must not redraw the login screen.
 */
export function useHubState<T>(selector: (state: HubState) => T, equals: (a: T, b: T) => boolean = Object.is): T {
  const store = useStore();
  const cache = useRef<{ state: HubState; selector: typeof selector; value: T } | null>(null);
  const read = useCallback((): T => {
    const state = store.getState();
    const last = cache.current;
    if (last && last.state === state && last.selector === selector) return last.value;
    const value = selector(state);
    const kept = last && equals(last.value, value) ? last.value : value;
    cache.current = { state, selector, value: kept };
    return kept;
  }, [store, selector, equals]);
  return useSyncExternalStore(store.subscribe, read, read);
}

const identity = (s: HubState): HubState => s;

/** The whole state: for screens that show most of it. Prefer a narrow selector where the screen needs little. */
export const useWholeState = (): HubState => useHubState(identity);
