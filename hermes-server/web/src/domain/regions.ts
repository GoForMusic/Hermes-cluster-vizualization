// Which region a cluster is in. Pure, so it is tested without a screen.
import type { Region } from './settings';

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
