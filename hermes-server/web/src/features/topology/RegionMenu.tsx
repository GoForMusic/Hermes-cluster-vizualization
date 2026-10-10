// The right-click menu of the admin map: put a cluster in a region, or rename, recolour and delete a region. Regions live in the hub's settings.
import { useState } from 'react';
import type { Region } from '../../domain/settings';
import { getNode } from '../../domain/selectors';
import { useStore, useWholeState } from '../../state/context';
import type { MapTarget } from './TopologyMap';

export const REGION_COLORS = ['#4aa3ff', '#3ddc97', '#ffb020', '#c084fc', '#ff6b6b', '#22d3ee'] as const;

const without = (regions: Region[], clusterId: string): Region[] =>
  regions.map((r) => ({ ...r, clusterIds: r.clusterIds.filter((id) => id !== clusterId) })).filter((r) => r.clusterIds.length); // an empty region is not drawn, so it would be unreachable

interface Props { target: MapTarget; at: { x: number; y: number }; onClose: () => void }

export function RegionMenu({ target, at, onClose }: Props) {
  const store = useStore();
  const state = useWholeState();
  const regions = state.settings.regions;
  const change = (next: (r: Region[]) => Region[]) => store.updateSettings((s) => ({ ...s, regions: next(s.regions) }));
  const [renaming, setRenaming] = useState<string | null>(null);
  const style = { left: Math.min(at.x, window.innerWidth - 240), top: Math.min(at.y, window.innerHeight - 260) };

  if (target.kind === 'cluster') {
    const name = getNode(state, target.id)?.name ?? target.id;
    const current = regions.find((r) => r.clusterIds.includes(target.id));
    const create = () => {
      const id = `rg${Date.now().toString(36)}`;
      change((all) => [...without(all, target.id), { id, name: 'New region', color: REGION_COLORS[all.length % REGION_COLORS.length]!, clusterIds: [target.id] }]);
      setRenaming(id);
    };
    if (renaming) return <RenameMenu id={renaming} at={style} onClose={onClose} />;
    const put = (r: Region) => { change((all) => without(all, target.id).map((x) => (x.id === r.id ? { ...x, clusterIds: [...x.clusterIds, target.id] } : x))); onClose(); };
    return (
      <div className="region-menu" style={style} onPointerDown={(e) => e.stopPropagation()}>
        <small className="muted" style={{ padding: '4px 10px' }}>{name}</small>
        <button onClick={create}>Add region…</button>
        {regions.filter((r) => r !== current).map((r) => <button key={r.id} onClick={() => put(r)}>Move to {r.name}</button>)}
        {current ? <button onClick={() => { change((all) => without(all, target.id)); onClose(); }}>Remove from {current.name}</button> : null}
        <div className="sep" />
        <button onClick={onClose}>Close</button>
      </div>
    );
  }
  return <RenameMenu id={target.id} at={style} onClose={onClose} full />;
}

function RenameMenu({ id, at, onClose, full = false }: { id: string; at: { left: number; top: number }; onClose: () => void; full?: boolean }) {
  const store = useStore();
  const region = useWholeState().settings.regions.find((r) => r.id === id);
  const [name, setName] = useState(region?.name ?? '');
  if (!region) return null;
  const patch = (p: Partial<Region>) => store.updateSettings((s) => ({ ...s, regions: s.regions.map((r) => (r.id === id ? { ...r, ...p } : r)) }));
  const save = () => { if (name.trim()) patch({ name: name.trim() }); onClose(); };
  return (
    <div className="region-menu" style={at} onPointerDown={(e) => e.stopPropagation()}>
      <input autoFocus value={name} onChange={(e) => setName(e.target.value)} onFocus={(e) => e.target.select()} onKeyDown={(e) => { if (e.key === 'Enter') save(); if (e.key === 'Escape') onClose(); }} aria-label="Region name" />
      <button onClick={save}>Save name</button>
      <div className="swatches">{REGION_COLORS.map((c) => <button key={c} aria-label={`Colour ${c}`} style={{ background: c, outline: region.color === c ? '2px solid var(--text)' : undefined }} onClick={() => patch({ color: c })} />)}</div>
      {full ? (
        <>
          <div className="sep" />
          <button className="danger" onClick={() => { store.updateSettings((s) => ({ ...s, regions: s.regions.filter((r) => r.id !== id) })); onClose(); }}>Delete region</button>
        </>
      ) : null}
    </div>
  );
}
