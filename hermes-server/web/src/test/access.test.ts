import { describe, expect, it } from 'vitest';
import { can, covers } from '../domain/access';

// the same cases as the hub's `tests/unit/policy.rs`: the page and the hub read a path the same way
describe('a path in a rule', () => {
  it.each([
    ['*', 'anything/at/all', true],
    ['sources', 'sources', true],
    ['sources', 'sources/s1', false],
    ['sources/*', 'sources/s1', true],
    ['sources/*', 'sources/s1/upgrade', true],
    ['sources/*', 'sources', true],
    ['sources/*', 'sourcesx', false],
    ['sources/prod-*', 'sources/prod-eu', true],
    ['sources/prod-*', 'sources/dev-eu', false],
    ['sources/prod-*', 'sources', false],
    ['alerts/+/ack', 'alerts/7/ack', true],
    ['alerts/+/ack', 'alerts/7/8/ack', false],
    ['dc/+/sources/*', 'dc/eu/sources/s1', true],
    ['users/3', '/users/3/', true],
  ])('%s covers %s: %s', (rule, path, yes) => {
    expect(covers(rule, path)).toBe(yes);
  });
});

describe('what the rules allow', () => {
  const grants = [
    { path: 'sources/*', capabilities: ['read', 'update'] },
    { path: 'sources/prod-*', capabilities: ['deny'] },
    { path: 'alerts', capabilities: ['list'] },
  ];
  it('adds the rules up, and deny wins', () => {
    expect(can(grants, 'read', 'sources/s1')).toBe(true);
    expect(can(grants, 'update', 'sources/s1')).toBe(true);
    expect(can(grants, 'delete', 'sources/s1')).toBe(false);
    expect(can(grants, 'read', 'sources/prod-eu')).toBe(false);
    expect(can(grants, 'read', 'settings')).toBe(false);
  });
  it('lets list through as a read, and no more', () => {
    expect(can(grants, 'read', 'alerts')).toBe(true);
    expect(can(grants, 'create', 'alerts')).toBe(false);
  });
  it('shows everything on a hub that sends no rules, and nothing to somebody with none', () => {
    expect(can(undefined, 'delete', 'x')).toBe(true);
    expect(can([], 'read', 'x')).toBe(false);
  });
});
