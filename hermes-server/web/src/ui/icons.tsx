// A small hand-drawn icon set (24x24, stroke based). Simplified: not the official logos. An icon is DATA (a list of shapes), so adding one is
// adding an entry; `Icon` draws it in HTML, `IconG` inside the map's SVG.
import type { ReactElement, SVGProps } from 'react';

type Shape = readonly [tag: 'path' | 'rect' | 'circle', attrs: SVGProps<SVGElement>];

const poly = (n: number, r: number, rot = -90): [number, number][] =>
  Array.from({ length: n }, (_, i) => {
    const a = ((rot + (i * 360) / n) * Math.PI) / 180;
    return [12 + r * Math.cos(a), 12 + r * Math.sin(a)];
  });
const pathOf = (pts: [number, number][]): string => 'M' + pts.map((p) => `${p[0].toFixed(2)} ${p[1].toFixed(2)}`).join('L') + 'Z';
const box = (x: number, y: number): Shape => ['rect', { x, y, width: 2.8, height: 2.8, rx: 0.4 }];
const dot = (cx: number, cy: number, r: number): Shape => ['circle', { cx, cy, r, fill: 'currentColor', stroke: 'none' }];

export const ICONS: Record<string, readonly Shape[]> = {
  // cluster orchestrators
  kubernetes: [['path', { d: pathOf(poly(7, 10)) }], ['circle', { cx: 12, cy: 12, r: 2.6 }], ...poly(7, 6.6).map(([x, y]): Shape => ['path', { d: `M12 12L${x.toFixed(2)} ${y.toFixed(2)}` }])],
  docker: [
    ['path', { d: 'M2.5 12.6h19c.9 0 1.7-.5 2-1.3-.7-.5-1.6-.4-2.2.1-.3-.9-1-1.5-1.9-1.7-.2.9 0 1.6.4 2.1' }],
    ['path', { d: 'M2.5 12.6c.3 3.6 3.1 6.1 7.6 6.1 4.8 0 8-2.5 9-6.1' }],
    box(4.6, 9.4), box(7.6, 9.4), box(10.6, 9.4), box(7.6, 6.4), box(10.6, 6.4), box(13.6, 9.4),
  ],
  nomad: [['path', { d: pathOf(poly(6, 10, -90)) }], ['path', { d: 'M9 9l3 3-3 3M13 9l3 3-3 3' }]],
  // things inside clusters
  server: [['rect', { x: 3.5, y: 4, width: 17, height: 6.5, rx: 1.5 }], ['rect', { x: 3.5, y: 13.5, width: 17, height: 6.5, rx: 1.5 }], dot(7.3, 7.25, 0.9), dot(7.3, 16.75, 0.9)],
  nas: [
    ['rect', { x: 3, y: 3.5, width: 18, height: 5, rx: 1.2 }], ['rect', { x: 3, y: 9.5, width: 18, height: 5, rx: 1.2 }], ['rect', { x: 3, y: 15.5, width: 18, height: 5, rx: 1.2 }],
    ['path', { d: 'M15.5 6h3M15.5 12h3M15.5 18h3' }], dot(6.4, 6, 0.8), dot(6.4, 12, 0.8), dot(6.4, 18, 0.8),
  ],
  // ways in: outside, an Ingress, a Service that fans out to pods, a load balancer
  globe: [['circle', { cx: 12, cy: 12, r: 9 }], ['path', { d: 'M3 12h18' }], ['path', { d: 'M12 3c2.6 2.5 4 5.5 4 9s-1.4 6.5-4 9c-2.6-2.5-4-5.5-4-9s1.4-6.5 4-9z' }]],
  gate: [['path', { d: 'M3 12h11M10 8l4 4-4 4M19 4v16' }]],
  fanout: [['path', { d: 'M3 12h5l5-6h7M8 12h12M8 12l5 6h7' }]],
  shield: [['path', { d: 'M12 3l8 3v6c0 4.5-3.2 7.8-8 9-4.8-1.2-8-4.5-8-9V6z' }]],
  balance: [['path', { d: 'M12 4v5M12 9L5 15M12 9l7 6M5 15v5M19 15v5' }]],
  net: [['path', { d: 'M4 8h14M14 4l4 4-4 4' }], ['path', { d: 'M20 16H6M10 12l-4 4 4 4' }]],
  cube: [['path', { d: 'M12 2.5l8.5 4.75v9.5L12 21.5l-8.5-4.75v-9.5z' }], ['path', { d: 'M12 12l8.5-4.75M12 12v9.5M12 12L3.5 7.25' }]],
  disk: [['path', { d: 'M4 6c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3z' }], ['path', { d: 'M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6' }], ['path', { d: 'M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3' }]],
  // infrastructure as code
  terraform: [
    ['path', { d: 'M3 3.5l6 3.5v7l-6-3.5z', fill: 'currentColor', stroke: 'none' }],
    ['path', { d: 'M10 7l6 3.5v7L10 14z', fill: 'currentColor', stroke: 'none' }],
    ['path', { d: 'M10 0.5l6 3.5v6.5L10 7z', fill: 'currentColor', stroke: 'none', opacity: 0.55 }],
    ['path', { d: 'M17 11l4.5 2.6v6.5L17 17.5z', fill: 'currentColor', stroke: 'none' }],
  ],
};

const shapes = (name: string): ReactElement[] =>
  (ICONS[name] ?? []).map(([tag, attrs], i) => {
    const Tag = tag as 'path';
    return <Tag key={i} {...(attrs as SVGProps<SVGPathElement>)} />;
  });

export function Icon({ name, size = 16, color = 'currentColor' }: { name: string; size?: number; color?: string }) {
  return (
    <svg className="ic" viewBox="0 0 24 24" width={size} height={size} fill="none" stroke={color} strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" style={{ color }}>
      {shapes(name)}
    </svg>
  );
}

/** The same icon as a `<g>` to embed in the map's SVG. */
export function IconG({ name, x = 0, y = 0, size = 16, color = 'currentColor' }: { name: string; x?: number; y?: number; size?: number; color?: string }) {
  return (
    <g transform={`translate(${x} ${y}) scale(${size / 24})`} fill="none" stroke="currentColor" strokeWidth={1.9} strokeLinecap="round" strokeLinejoin="round" style={{ color }}>
      {shapes(name)}
    </g>
  );
}
