import type { Own, Status } from './model';

export const RANK: Record<Status, number> = { ok: 0, unknown: 1, warn: 2, crit: 3 };
export const ICON: Record<Status, string> = { ok: '✓', warn: '!', crit: '✕', unknown: '?' };
export const LABEL: Record<Status, string> = { ok: 'Healthy', warn: 'Warning', crit: 'Critical', unknown: 'Unknown' };
export const STATUS_WORD: Record<Status, string> = { ok: 'Up', warn: 'Degraded', crit: 'Down', unknown: 'Unreachable' };

export const isOwn = (s: string): s is Own => s === 'ok' || s === 'warn' || s === 'crit';
/** Is the node running, as far as anybody knows? */
export const isRunning = (s: Status): boolean => s === 'ok' || s === 'warn';
