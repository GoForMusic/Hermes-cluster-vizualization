// The wallboard's cluster tabs: one scrollable row with < > arrows (the active tab scrolls into view when the rotation moves on),
// plus a searchable list for when there are too many clusters to page through.
import { useEffect, useRef, useState } from 'react';
import { PROVIDERS } from '../../domain/providers';
import { Icon } from '../../ui/icons';

interface Cluster { id: string; name: string; status: string; provider: string }
interface Props { clusters: Cluster[]; focusId: string | null; onChoose: (id: string | null) => void }

const ALL = { id: null, name: 'All clusters', status: null, provider: null };

export function ClusterTabs({ clusters, focusId, onChoose }: Props) {
  const row = useRef<HTMLDivElement>(null);
  const [edge, setEdge] = useState({ start: true, end: true });
  const [menu, setMenu] = useState(false);
  const [q, setQ] = useState('');

  const measure = () => {
    const el = row.current;
    if (el) setEdge({ start: el.scrollLeft <= 1, end: el.scrollLeft + el.clientWidth >= el.scrollWidth - 1 });
  };
  useEffect(() => {
    measure();
    const el = row.current;
    if (!el) return;
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [clusters.length]);
  useEffect(() => { row.current?.querySelector('.pill.on')?.scrollIntoView?.({ block: 'nearest', inline: 'center' }); }, [focusId]);

  const page = (dir: 1 | -1) => row.current?.scrollBy?.({ left: dir * row.current.clientWidth * 0.7 });
  const pill = (c: typeof ALL | Cluster, after?: () => void) => {
    const prov = c.provider ? PROVIDERS[c.provider as keyof typeof PROVIDERS] : null;
    return (
      <button key={c.id ?? 'all'} className={`pill${focusId === c.id ? ' on' : ''}${c.status ? ` st-${c.status}` : ''}`} title={prov ? prov.label : 'Show everything'} onClick={() => { onChoose(c.id); after?.(); }}>
        {c.status ? <span className="dot" /> : null}
        {prov ? <Icon name={prov.icon} size={14} color={prov.color} /> : null}
        {c.name}
      </button>
    );
  };
  const found = clusters.filter((c) => c.name.toLowerCase().includes(q.trim().toLowerCase()));

  return (
    <div className="tv-tabs">
      <button className="arrow" aria-label="Scroll left" disabled={edge.start} onClick={() => page(-1)}>‹</button>
      <div className="tv-pills" ref={row} onScroll={measure}>{[ALL, ...clusters].map((c) => pill(c))}</div>
      <button className="arrow" aria-label="Scroll right" disabled={edge.end} onClick={() => page(1)}>›</button>
      <button className="pick" aria-label="Find a cluster" onClick={() => setMenu((m) => !m)}>{clusters.length} ▾</button>
      {menu ? (
        <div className="tab-menu">
          <input autoFocus placeholder="Find a cluster…" value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === 'Escape') setMenu(false); }} />
          <div className="list">{[ALL, ...found].filter((c) => c.id === null ? !q : true).map((c) => pill(c, () => { setMenu(false); setQ(''); }))}</div>
        </div>
      ) : null}
    </div>
  );
}
