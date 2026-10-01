// The big amber/red banner over the map: the current critical alert, or the warnings, or nothing.
export function Banner({ crit, warn }: { crit: { title: string; detail: string }[]; warn: { title: string }[] }) {
  if (crit.length) {
    const first = crit[0]!;
    return (
      <div className="banner crit show">
        <span className="b-tag">Critical</span>
        <div className="b-title">{first.title}</div>
        <div className="b-detail">{first.detail}{crit.length > 1 ? `  ·  +${crit.length - 1} more` : ''}</div>
      </div>
    );
  }
  if (warn.length) return <div className="banner warn show"><span className="b-tag">Warning</span><div className="b-title">{warn.map((a) => a.title).join('  ·  ')}</div></div>;
  return <div className="banner" />;
}
