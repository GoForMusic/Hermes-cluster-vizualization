// The coordinate grid behind the map (minor/major lines, letter/number labels), like a tactical map's reference grid.
import { useMemo } from 'react';
import { GRID_MARGIN, GRID_SIZE } from '../../domain/map/links';

export function Grid({ w, h }: { w: number; h: number }) {
  const { minor, major, cols, rows } = useMemo(() => {
    const x0 = -GRID_MARGIN, y0 = -GRID_MARGIN;
    const cols = Math.ceil((w + GRID_MARGIN * 2) / GRID_SIZE), rows = Math.ceil((h + GRID_MARGIN * 2) / GRID_SIZE);
    const step = GRID_SIZE / 4;
    let minor = '', major = '';
    for (let i = 0; i <= cols * 4; i++) { const x = x0 + i * step, seg = `M${x} ${y0}V${y0 + rows * GRID_SIZE}`; if (i % 4 === 0) major += seg; else minor += seg; }
    for (let j = 0; j <= rows * 4; j++) { const y = y0 + j * step, seg = `M${x0} ${y}H${x0 + cols * GRID_SIZE}`; if (j % 4 === 0) major += seg; else minor += seg; }
    return { minor, major, cols, rows };
  }, [w, h]);
  const x0 = -GRID_MARGIN, y0 = -GRID_MARGIN;
  return (
    <g>
      <path className="grid-minor" d={minor} />
      <path className="grid-major" d={major} />
      {Array.from({ length: cols }, (_, c) => <text key={`c${c}`} className="grid-label" x={x0 + c * GRID_SIZE + GRID_SIZE / 2} y={y0 + 16} textAnchor="middle">{String.fromCharCode(65 + (c % 26))}</text>)}
      {Array.from({ length: rows }, (_, r) => <text key={`r${r}`} className="grid-label" x={x0 + 8} y={y0 + r * GRID_SIZE + GRID_SIZE / 2 + 4}>{String(r + 1).padStart(2, '0')}</text>)}
    </g>
  );
}
