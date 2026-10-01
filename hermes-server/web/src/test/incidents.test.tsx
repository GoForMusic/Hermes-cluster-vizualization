import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { incidentFacts, incidentMarkdown, incidentsMarkdown, parseSnapshot, snapshotFacts, sourceOf } from '../domain/incidents';
import { initialState } from '../domain/hubState';
import type { Alert } from '../domain/model';
import { boot, reduce } from '../domain/reducer';
import { Alerts } from '../features/admin/pages/Alerts';
import { NOW, wireNode } from './fixtures';
import { renderWithHub } from './render';

const SNAP = JSON.stringify({ own: 'crit', reason: 'ImagePullBackOff', m: { cpuMilli: 12, memMiB: 40 }, meta: { type: 'Deployment', ns: 'shop', image: 'shop/api:2.1', restarts: 4, phase: 'Pending', containers: [{ name: 'api', image: 'shop/api:2.1', ready: false, state: 'waiting: ImagePullBackOff', restarts: 4 }] } });

const alert = (id: number, over: Partial<Alert> = {}): Alert => ({ id, key: `k${id}`, sev: 'crit', nodeId: 's1:w:api', title: 'Pod failing', detail: 'shop/api is in ImagePullBackOff', ts: NOW - 3_600_000, resolvedTs: NOW - 1_800_000, ack: true, ackBy: 'admin', ackTs: NOW - 3_000_000, snapshot: SNAP, ...over });

function state(alerts: Alert[], withNode = true) {
  const nodes = [
    wireNode('s1:c', 'cluster', null, { name: 'prod' }),
    wireNode('s1:h', 'host', 's1:c', { name: 'node-a' }),
    ...(withNode ? [wireNode('s1:w:api', 'workload', 's1:h', { name: 'api-7d9' })] : []),
  ];
  let s = boot(initialState(), { nodes, edges: [], info: { demo: false, sourceTypes: [], version: '1' }, settings: {}, alerts: [], sources: [] }, NOW);
  for (const a of alerts) s = reduce(s, { type: 'alert', alert: a, isNew: true }, NOW).state;
  return s;
}

describe('an incident report', () => {
  it('tells where it happened, how long, who acknowledged it and what the pod looked like then', () => {
    const a = alert(1);
    const f = incidentFacts(state([a]), a, NOW);
    expect(f.path).toEqual(['prod', 'node-a', 'api-7d9']);
    expect(f.duration).toBe(1_800_000);
    expect(f.active).toBe(false);
    const facts = Object.fromEntries(snapshotFacts(f.snapshot!));
    expect(facts).toMatchObject({ 'Status then': 'crit', 'Reason then': 'ImagePullBackOff', Image: 'shop/api:2.1', Restarts: '4', Namespace: 'shop', 'CPU then': '12m', 'Memory then': '40 MiB' });
  });

  it('still tells the story when the pod is gone', () => {
    const a = alert(1);
    const f = incidentFacts(state([a], false), a, NOW);
    expect(f.gone).toBe(true);
    expect(f.path).toEqual(['s1:w:api']);
    expect(f.snapshot?.reason).toBe('ImagePullBackOff'); // the node is gone, what it said is not
  });

  it("lists what else of the same source was down meanwhile, and nothing of another source or another time", () => {
    const a = alert(1, { resolvedTs: NOW - 1_000_000 });
    const during = alert(2, { nodeId: 's1:h', title: 'Host unreachable', ts: NOW - 3_000_000, resolvedTs: NOW - 2_000_000 });
    const later = alert(3, { nodeId: 's1:h', title: 'Later', ts: NOW - 500_000, resolvedTs: null });
    const other = alert(4, { nodeId: 's2:h', title: 'Elsewhere', ts: NOW - 3_000_000 });
    const s = state([a, during, later, other]);
    expect(incidentFacts(s, a, NOW).overlapping.map((o) => o.id)).toEqual([2]);
  });

  it('is an open incident while it has no end', () => {
    const a = alert(1, { resolvedTs: null, ack: false, ackBy: undefined, ackTs: undefined });
    const f = incidentFacts(state([a]), a, NOW);
    expect(f.active).toBe(true);
    expect(f.duration).toBe(3_600_000);
    expect(incidentMarkdown(a, f)).toContain('**Still open:**');
    expect(incidentMarkdown(a, f)).toContain('**Acknowledged:** no');
  });

  it('exports as Markdown that a ticket can hold', () => {
    const a = alert(1);
    const md = incidentMarkdown(a, incidentFacts(state([a]), a, NOW));
    expect(md).toContain('## CRITICAL: Pod failing');
    expect(md).toContain('**Where:** prod › node-a › api-7d9');
    expect(md).toContain('**Acknowledged:** admin');
    expect(md).toContain('- Image: shop/api:2.1');
    expect(md).toContain('- api (shop/api:2.1): waiting: ImagePullBackOff, 4 restarts, not ready');
    const all = incidentsMarkdown([alert(2, { ts: NOW - 100 }), a], state([a]), NOW, 'Incident report');
    expect(all.startsWith('# Incident report')).toBe(true);
    expect(all).toContain('2 incidents (2 critical, 0 warning)');
    expect(all.indexOf('Pod failing')).toBeLessThan(all.lastIndexOf('Pod failing'));
  });

  it('survives a snapshot that is missing or broken, and reads a source from an id', () => {
    expect(parseSnapshot('')).toBeNull();
    expect(parseSnapshot('{nope')).toBeNull();
    expect(sourceOf('s1:n:host')).toBe('s1');
    expect(sourceOf('source:s1')).toBe('s1');
  });
});

describe('the alerts page', () => {
  // the page judges "the last 7 days" by the real clock, so the incidents are moved from the fixed test time to now
  const shift = Date.now() - NOW;
  const now = (a: Alert): Alert => ({ ...a, ts: a.ts + shift, resolvedTs: a.resolvedTs == null ? null : a.resolvedTs + shift, ackTs: a.ackTs == null ? undefined : a.ackTs + shift });
  const open = async (alerts: Alert[]) => {
    const rig = await renderWithHub(<Alerts />, { prepare: (h) => { h.alerts = alerts.map(now); } });
    return { ...rig, user: userEvent.setup() };
  };

  it('opens the report of an incident from its title', async () => {
    const { user } = await open([alert(1)]);
    await user.click(await screen.findByRole('button', { name: 'Pod failing' }));
    expect(await screen.findByText('What the hub saw')).toBeInTheDocument();
    expect(screen.getByText('shop/api is in ImagePullBackOff', { selector: '.report-detail' })).toBeInTheDocument();
    expect(screen.getByText('The node when it opened')).toBeInTheDocument();
    expect(screen.getByText(/acknowledged/i, { selector: 'dt' })).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Copy as Markdown' })).toHaveLength(2); // the page's, and the report's
  });

  it('filters by severity, by words and by period, and says how many are left', async () => {
    const day = 86_400_000;
    const { user } = await open([
      alert(1, { title: 'Pod failing', sev: 'crit' }),
      alert(2, { title: 'Volume nearly full', sev: 'warn', nodeId: 's1:h', detail: 'data 91%' }),
      alert(3, { title: 'Old one', ts: NOW - 20 * day, resolvedTs: NOW - 19 * day }),
    ]);
    expect(await screen.findByText('2 incidents')).toBeInTheDocument(); // 7d: the old one is out of the period
    await user.selectOptions(screen.getByLabelText('Severity'), 'warn');
    expect(screen.getByText('1 incident')).toBeInTheDocument();
    expect(screen.getByText('Volume nearly full')).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText('Severity'), '');
    await user.type(screen.getByLabelText('Search'), 'data 91');
    expect(screen.getByText('1 incident')).toBeInTheDocument();
    await user.clear(screen.getByLabelText('Search'));
    await user.click(screen.getByRole('button', { name: '30d' }));
    expect(screen.getByText('3 incidents')).toBeInTheDocument();
  });

  it('copies what is shown as a Markdown report', async () => {
    const { user } = await open([alert(1)]);
    await user.click(await screen.findByRole('button', { name: 'Copy as Markdown' }));
    const copied = await navigator.clipboard.readText(); // the test's clipboard
    expect(copied).toContain('# Incident report · last 7d');
    expect(copied).toContain('## CRITICAL: Pod failing');
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeInTheDocument();
  });

  it('says so when nothing matches', async () => {
    const { user } = await open([alert(1)]);
    await user.type(await screen.findByLabelText('Search'), 'zzz');
    expect(screen.getByText('Nothing here')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy as Markdown' })).toBeDisabled();
  });
});
