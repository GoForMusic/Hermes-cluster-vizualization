import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { NETWORK_KINDS, Legend } from '../features/topology/Legend';
import { NETWORK_ICONS } from '../domain/mapLabels';

describe('the legend', () => {
  it('has a row for every icon a network can have on the map', () => {
    render(<Legend open onOpenChange={() => undefined} />);
    for (const k of NETWORK_KINDS) expect(screen.getByText(k.title)).toBeInTheDocument();
    const drawn = new Set(Object.values(NETWORK_ICONS));
    const explained = new Set<string>(NETWORK_KINDS.map((k) => k.icon));
    for (const icon of drawn) expect(explained.has(icon), `the icon "${icon}" is on the map but not in the legend`).toBe(true);
  });
});
