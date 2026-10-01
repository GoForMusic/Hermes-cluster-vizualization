// A small inline line chart with no axes, for a trend at a glance.
export function Sparkline({ values, width = 120, height = 32, color = 'var(--accent)' }: { values: readonly number[]; width?: number; height?: number; color?: string }) {
  if (values.length < 2) return <svg className="spark" viewBox={`0 0 ${width} ${height}`} width={width} height={height} aria-hidden="true" />;
  const min = Math.min(...values);
  const span = Math.max(...values) - min || 1;
  const pts = values.map((v, i): [number, number] => [(i / (values.length - 1)) * width, height - 3 - ((v - min) / span) * (height - 6)]);
  const line = pts.map((p, i) => `${i ? 'L' : 'M'}${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join(' ');
  return (
    <svg className="spark" viewBox={`0 0 ${width} ${height}`} width={width} height={height} aria-hidden="true">
      <path d={`${line} L${width} ${height} L0 ${height} Z`} fill={color} opacity={0.12} />
      <path d={line} fill="none" stroke={color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
