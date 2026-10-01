import { useEffect, useState, type FormEvent } from 'react';

/** What every form shows for an error that wasn't its own: the message of an `Error`, or the thing itself, stringified. */
export const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export interface AsyncSubmit {
  busy: boolean;
  error: string;
  submit: (e: FormEvent) => void;
}

/**
 * The busy/error/submit shape every admin form repeats: an optional synchronous `guard` runs first (return a message to stop there,
 * before the busy indicator ever shows — also the place to clear anything the form shows besides `error`); otherwise `work` runs with
 * `busy` up, and a thrown error becomes `error`. What `work` did on success (close a dialog, show a message, navigate away) is its own
 * business — this hook only owns `busy` and `error`.
 */
export function useAsyncSubmit(work: () => Promise<void>, guard?: () => string | null): AsyncSubmit {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const submit = (e: FormEvent) => {
    e.preventDefault();
    setError('');
    const problem = guard?.();
    if (problem) { setError(problem); return; }
    setBusy(true);
    void (async () => {
      try { await work(); } catch (ex) { setError(errorText(ex)); } finally { setBusy(false); }
    })();
  };
  return { busy, error, submit };
}

/** The current time, refreshed every `everyMs`: what "5s ago" and the clock are drawn from. */
export function useNow(everyMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), everyMs);
    return () => clearInterval(t);
  }, [everyMs]);
  return now;
}

/** A place in the hash of the address: `#/admin/dashboard/h1` is view `admin`, page `dashboard`, arg `h1`. */
export interface HashRoute {
  view: string;
  page: string;
  arg: string;
}

export const parseHash = (hash: string): HashRoute => {
  const [view = '', page = '', arg = ''] = hash.replace(/^#\/?/, '').split('/');
  return { view, page, arg };
};

export function useHashRoute(): HashRoute {
  const [route, setRoute] = useState(() => parseHash(location.hash));
  useEffect(() => {
    const on = () => setRoute(parseHash(location.hash));
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return route;
}
