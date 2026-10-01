// A traffic/control/route line between two map symbols: the path, its arrowhead, its animated particles and its label.
import { memo } from 'react';
import type { LinkGeometry } from '../../domain/map/links';
import { arrowPath } from '../../domain/map/links';
import { fmtMbps } from '../../domain/format';
import type { Edge } from '../../domain/model';
import { linkStrokeWidth, particleCount } from '../../domain/mapLabels';

export const MapLink = memo(function MapLink({ pathId, edge, color, approx, geometry, broken, related }: { pathId: string; edge: Edge; color?: string | undefined; approx?: boolean; geometry: LinkGeometry; broken: boolean; related: boolean }) {
  const control = edge.type === 'control';
  const route = edge.type === 'route';
  const v = edge.mbps;
  const measured = route && v >= 0.05; // a route that carries traffic somebody measured (the flows agent) is drawn like a traffic link
  const count = route && !measured ? 0 : particleCount(v, control, broken);
  const label = broken || (route && !measured) ? '' : route ? `${approx ? '≈ ' : ''}${fmtMbps(v)}` : control ? (v > 0.05 ? `control · ${fmtMbps(v)}` : 'control') : v < 5 ? '' : fmtMbps(v);
  const DUR = 3.2;
  return (
    <g className={`gedge${control ? ' control' : ''}${route ? ' route' : ''}${route && color ? ' net' : ''}${broken ? ' broken' : ''}${related ? ' rel' : ''}`}>
      <path id={pathId} className="link" d={geometry.d} strokeWidth={route && !measured ? 1.4 : linkStrokeWidth(v, control, broken)} style={color && !broken ? { stroke: color } : undefined} />
      <path className="arrow" d={arrowPath(geometry.end)} style={color && !broken ? { fill: color } : undefined} />
      <g key={`${broken}:${count}`}>
        {Array.from({ length: count }, (_, i) => (
          <circle key={i} className="particle" r={2.6}>
            <animateMotion dur={`${DUR}s`} begin={`${-(i * DUR) / count}s`} repeatCount="indefinite"><mpath href={`#${pathId}`} /></animateMotion>
          </circle>
        ))}
      </g>
      {/* the label sits ON the line (its halo hides the line under it), so it never lands on a neighbouring element */}
      <text className="link-label" x={geometry.mid.x} y={geometry.mid.y + 3.5} textAnchor="middle">{label}</text>
    </g>
  );
});
