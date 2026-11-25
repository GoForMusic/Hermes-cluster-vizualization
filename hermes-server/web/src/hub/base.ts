/**
 * Where the hub is mounted. The hub puts `window.__HUB_BASE__` into `index.html` (`HUB_BASE_PATH`, e.g. `/hermes`; empty = the root),
 * so one build serves under any path. In development (Vite serves the page) nothing sets it and the base is empty.
 */
declare global {
  interface Window {
    __HUB_BASE__?: string;
  }
}

export const BASE: string = (typeof window !== 'undefined' && window.__HUB_BASE__) || '';

/** `/api/x` -> `/hermes/api/x`. */
export const withBase = (path: string): string => BASE + path;

/** The address an agent or another hub uses to reach this one: the page's origin plus the base path. */
export const hubAddress = (): string => location.origin + BASE;
