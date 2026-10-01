// A workload/volume/network's diamond-or-rectangle unit symbol (`MapNode`), and the plate shown instead of the networks a cluster
// has too many of to draw (`MoreNetworks`).
import { memo, type MouseEvent } from 'react';
import type { Placed } from '../../domain/map/layout';
import type { Node } from '../../domain/model';
import { networkIcon, nodeNet, nodeSub, shorten, volumeBadge, volumeFill } from '../../domain/mapLabels';
import { ICON } from '../../domain/status';
import { IconG } from '../../ui/icons';

export const MapNode = memo(function MapNode({ node: n, at, system, selected, related, tooltip, onClick, color }: { node: Node; at: Placed; system: boolean; selected: boolean; related: boolean; tooltip: string; onClick: (e: MouseEvent) => void; color?: string | undefined }) {
  const isNetwork = n.kind === 'network';
  const isVolume = n.kind === 'volume';
  const fill = volumeFill(n);
  return (
    <g className={`gnode ${n.kind} st-${n.status}${system ? ' sys' : ''}${selected ? ' sel' : ''}${related ? ' rel' : ''}`} data-id={n.id} transform={`translate(${at.x} ${at.y})`} onClick={onClick}>
      <polygon className="ring" points="0,-30 34,0 0,30 -34,0" />
      {isNetwork ? <polygon className="sym sym-rect" points="-20,0 -10,-14 10,-14 20,0 10,14 -10,14" /> : <rect className="sym sym-rect" x={-27} y={-19} width={54} height={38} />}
      {isNetwork && color ? <polygon className="net-ring" points="-25,0 -12.5,-17.5 12.5,-17.5 25,0 12.5,17.5 -12.5,17.5" style={{ stroke: color }} /> : null}
      <polygon className="sym sym-dia" points="0,-30 34,0 0,30 -34,0" />
      {isNetwork ? <IconG name={networkIcon(n)} x={-8} y={-8} size={16} color="var(--ink)" /> : isVolume ? (
        <>
          <rect className="vol-fill" x={-26} width={52} y={18 - 36 * fill} height={36 * fill} />
          <IconG name="disk" x={-23} y={-8} size={16} color="var(--ink)" />
          <text className="vol-pct" x={9} y={4.5} textAnchor="middle">{volumeBadge(n)}</text>
        </>
      ) : <IconG name="cube" x={-11} y={-11} size={22} color="var(--ink)" />}
      <path className="sel-mark" d="M-38 -14V-25H-27M27 -25H38V-14M38 14V25H27M-27 25H-38V14" />
      <title>{tooltip}</title>
      <g className="badge">
        <rect className="badge-bg" x={20} y={-27} width={14} height={14} />
        <text className="badge-tx" x={27} y={-16.5} textAnchor="middle">{ICON[n.status]}</text>
      </g>
      <text className="n-name" y={36} textAnchor="middle">{shorten(n.name, 15)}</text>
      <text className="n-sub" y={49} textAnchor="middle">{nodeSub(n)}</text>
      <text className="n-net" y={61} textAnchor="middle">{nodeNet(n)}</text>
    </g>
  );
});

/** The plate for the networks a cluster has more of than the map can draw. */
export function MoreNetworks({ at, count }: { at: Placed; count: number }) {
  return (
    <g className="gnode network more" transform={`translate(${at.x} ${at.y})`}>
      <title>{`${count} more networks (Services, policies...) are not drawn: the map shows the first ones. They are all in the Admin panel.`}</title>
      <polygon className="sym sym-rect" points="-20,0 -10,-14 10,-14 20,0 10,14 -10,14" />
      <text className="vol-pct" y={4.5} textAnchor="middle">+{count}</text>
      <text className="n-name" y={36} textAnchor="middle">MORE</text>
      <text className="n-sub" y={49} textAnchor="middle">not drawn</text>
    </g>
  );
}
