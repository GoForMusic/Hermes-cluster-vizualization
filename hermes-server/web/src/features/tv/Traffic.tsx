// The busiest connections right now: who asked whom, and how much moved. Only when a flows agent reports.
import { fmtMbps } from '../../domain/format';
import { topFlows } from '../../domain/selectors';
import { useWholeState } from '../../state/context';
import { useNow } from '../../ui/hooks';

export function Traffic() {
  const now = useNow(1000);
  const state = useWholeState();
  const lines = topFlows(state, now);
  if (!lines.length) return null;
  const name = (id: string, external: boolean): string => state.nodes.get(id)?.name ?? (external ? `outside · ${id}` : id);
  return (
    <section className="panel">
      <div className="panel-h">Traffic<em>busiest now</em></div>
      <div className="panel-b flows">
        {lines.map((l, i) => (
          <div key={i} className="fl" title={`${l.port ? `port ${l.port}` : ''}${l.via ? ` · answered by ${name(l.via, false)}` : ''}`}>
            <span className="fl-who">{name(l.src, l.external)}<i> → </i>{name(l.dst, false)}</span>
            <b>{fmtMbps(l.mbps)}</b>
          </div>
        ))}
      </div>
    </section>
  );
}
