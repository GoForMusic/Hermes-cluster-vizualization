// A host's box on the map: the plain frame drawn under the links (`MapHost`), and its text/icons drawn over them afterwards
// (`HostLabel`) so a line passing behind a name never runs through it.
import { memo, type MouseEvent, type PointerEvent as ReactPointerEvent } from 'react';
import { CELL_W, MAX_COLS, type HostBox } from '../../domain/map/layout';
import type { Node } from '../../domain/model';
import { PROVIDERS, TF_COLOR } from '../../domain/providers';
import { IconG } from '../../ui/icons';

const clip = (text: string, max: number): string => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

/** The box of a host. Its text is drawn later (`HostLabel`), over the links, so that a line passing behind a name does not run through it. */
export const MapHost = memo(function MapHost({ box, host, selected, onClick }: { box: HostBox; host: Node; selected: boolean; onClick: (e: MouseEvent) => void }) {
  const isNas = host.meta.role === 'storage';
  return (
    <g className={`ghost st-${host.status}${selected ? ' sel' : ''}`} data-id={host.id} onClick={onClick}>
      <title>{isNas ? 'NAS — a storage box on the network. Clusters mount its folders over NFS.' : `Host — a ${host.meta.role} machine (VM or PC) that runs workloads.${host.meta.location ? `\nLocation: ${host.meta.location}` : ''}`}</title>
      <rect className="host-box" x={box.x} y={box.y} width={box.w} height={box.h} />
      <rect className="host-strip" x={box.x + 0.6} y={box.y + 0.6} width={box.w - 1.2} height={27} />
    </g>
  );
});

export const HostLabel = memo(function HostLabel({ box, host, stat, selected, onClick }: { box: HostBox; host: Node; stat: string; selected: boolean; onClick: (e: MouseEvent) => void }) {
  const isNas = host.meta.role === 'storage';
  const iac = host.meta.iac;
  const tfW = iac?.drift ? 60 : 78;
  const tfX = box.x + box.w - 10 - tfW, tfY = box.y + 46;
  return (
    <g className={`ghost st-${host.status}${selected ? ' sel' : ''}`} data-id={host.id} onClick={onClick}>
      <IconG name={isNas ? 'nas' : 'server'} x={box.x + 8} y={box.y + 5} size={18} color={PROVIDERS[host.provider].color} />
      <text className="host-name" x={box.x + 32} y={box.y + 19}>{host.name}</text>
      {host.meta.osType ? <text className={`host-os ${host.meta.osType}`} x={box.x + box.w - 10} y={box.y + 19} textAnchor="end">{host.meta.osType.toUpperCase()}</text> : null}
      {iac ? (
        <g className="tf">
          <title>{iac.drift ? `Terraform drift: the real machine no longer matches the Terraform code. ${iac.note}` : `Managed by Terraform (${iac.ref}). The real machine matches the code.`}</title>
          <rect className={iac.drift ? 'tf-bg drift' : 'tf-bg'} x={tfX} y={tfY} width={tfW} height={17} />
          <IconG name="terraform" x={tfX + 5} y={tfY + 3} size={11} color={iac.drift ? 'var(--warn)' : TF_COLOR} />
          <text className={iac.drift ? 'tf-tx drift' : 'tf-tx'} x={tfX + 20} y={tfY + 12.5}>{iac.drift ? 'drift' : 'managed'}</text>
        </g>
      ) : null}
      <text className="host-sub" x={box.x + 10} y={box.y + 44}>{host.meta.ip} · {host.meta.role}{host.meta.location ? ` · ${clip(host.meta.location, 22)}` : ''}</text>
      <text className="host-stat" x={box.x + 10} y={box.y + 59}>{stat}</text>
    </g>
  );
});

/**
 * The handle in the bottom-right corner of a host's box: dragging it sideways changes how many columns the workloads are laid out in
 * (wider and flatter, or narrower and taller); a double click goes back to the automatic layout.
 */
export const HostGrip = memo(function HostGrip({ box, scale, onHold, onCols }: { box: HostBox; scale: number; onHold: () => void; onCols: (cols: number | null) => void }) {
  const start = (e: ReactPointerEvent<SVGGElement>) => {
    if (e.button !== 0) return; // the middle button still moves the map
    e.stopPropagation(); // not a pan of the map
    e.preventDefault();
    onHold(); // the map stays where it is while the box changes size
    const x0 = e.clientX, cols0 = box.cols, max = Math.min(MAX_COLS, Math.max(box.items.length, 1));
    const move = (ev: PointerEvent) => onCols(Math.min(Math.max(Math.round(cols0 + (ev.clientX - x0) / scale / CELL_W), 1), max));
    const up = () => { window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', up); };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  };
  const x = box.x + box.w - 5, y = box.y + box.h - 5;
  return (
    <g className="host-grip" onPointerDown={start} onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => { e.stopPropagation(); onCols(null); }}>
      <title>Drag sideways to change the width of this box (how many columns). Double click: automatic.</title>
      <rect x={x - 16} y={y - 16} width={20} height={20} fill="transparent" />
      <path d={`M${x - 3} ${y - 12} L${x - 12} ${y - 3} M${x - 3} ${y - 7} L${x - 7} ${y - 3}`} />
    </g>
  );
});
