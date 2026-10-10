// Which region a cluster is in. Pure, so it is tested without a screen.
import { UNGROUPED } from './map/layout';
import type { Region, Settings } from './settings';

/** `regions` with the cluster in `regionId` (`null`: in none). A region left without clusters is gone: it would not be drawn, so it could not be reached. */
export function assign(regions: readonly Region[], clusterId: string, regionId: string | null): Region[] {
  return regions
    .map((r) => {
      const rest = r.clusterIds.filter((id) => id !== clusterId);
      return { ...r, clusterIds: r.id === regionId ? [...rest, clusterId] : rest };
    })
    .filter((r) => r.clusterIds.length);
}

export const regionOf = (regions: readonly Region[], clusterId: string): string | null => regions.find((r) => r.clusterIds.includes(clusterId))?.id ?? null;

/** The settings with new regions, and the order of the regions cleaned of ones that are gone: with none left it is automatic again. */
export function withRegions(s: Settings, regions: Region[]): Settings {
  const known = new Set([...regions.map((r) => r.id), UNGROUPED]);
  const rows = s.regionRows.map((row) => row.filter((id) => known.has(id))).filter((row) => row.length);
  return { ...s, regions, regionRows: regions.length ? rows : [] };
}
