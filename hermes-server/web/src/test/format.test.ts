import { describe, expect, it } from 'vitest';
import { dtg, fmtCpu, fmtDur, fmtMbps, fmtMem, fmtRateShort, fmtSize, timeAgo } from '../domain/format';
import { DEFAULT_RULES, mergeSettings } from '../domain/settings';

describe('formats', () => {
  it('sizes, cpu and memory', () => {
    expect([fmtSize(0.5), fmtSize(9.96), fmtSize(120), fmtSize(1536)]).toEqual(['0.5 GiB', '10.0 GiB', '120 GiB', '1.50 TiB']);
    expect([fmtCpu(12), fmtCpu(1500), fmtMem(512), fmtMem(2048)]).toEqual(['12m', '1.50 cores', '512 MiB', '2.0 GiB']);
  });

  it('rates', () => {
    expect([fmtRateShort(0), fmtRateShort(0.012), fmtRateShort(1.24), fmtRateShort(42)]).toEqual(['0', '12K', '1.2M', '42M']);
    expect([fmtMbps(0.5), fmtMbps(3.14), fmtMbps(42), fmtMbps(2500)]).toEqual(['500 Kb/s', '3.1 Mb/s', '42 Mb/s', '2.5 Gb/s']);
  });

  it('times', () => {
    const now = 1_000_000;
    expect([timeAgo(now - 5_000, now), timeAgo(now - 120_000, now), timeAgo(now - 7_200_000, now), timeAgo(now + 5_000, now)]).toEqual(['5s', '2m', '2h', '0s']);
    expect([fmtDur(45_000), fmtDur(125_000), fmtDur(7_260_000)]).toEqual(['45s', '2m 5s', '2h 1m']);
    expect(dtg(new Date(Date.UTC(2026, 8, 20, 14, 53)))).toBe('201453Z SEP 26');
  });
});

describe('settings', () => {
  it('start from the defaults and take what was saved on top', () => {
    const s = mergeSettings({ rotateSec: 45, rotate: true, clusters: { a: false }, rules: [{ id: 'volume-usage', value: 70 }] });
    expect([s.rotateSec, s.rotate, s.sidebar, s.clusters]).toEqual([45, true, true, { a: false }]);
    expect(s.rules.find((r) => r.id === 'volume-usage')).toMatchObject({ value: 70, crit: 95, enabled: true });
    expect(s.rules).toHaveLength(DEFAULT_RULES.length);
  });

  it('survive garbage', () => {
    for (const bad of [null, undefined, 3, 'x']) expect(mergeSettings(bad).rotateSec).toBe(20);
  });
});
