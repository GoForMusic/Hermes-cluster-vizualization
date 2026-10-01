// The incident log: opened and resolved alerts, newest first, terminal-style.
import { hhmmss } from '../../domain/format';
import { useHubState } from '../../state/context';

export function Console() {
  const alerts = useHubState((s) => s.alerts);
  const events = alerts.slice(0, 40).flatMap((a) => [
    { t: a.ts, cls: a.sev === 'crit' ? 'crit' : 'warn', tag: a.sev === 'crit' ? 'CRIT' : 'WARN', text: a.title, detail: a.resolvedTs != null ? null : a.detail },
    ...(a.resolvedTs != null ? [{ t: a.resolvedTs, cls: 'ok', tag: ' OK ', text: `${a.title} — resolved`, detail: null }] : []),
  ]).sort((x, y) => y.t - x.t).slice(0, 40);
  return (
    <div className="console">
      {events.map((e, i) => (
        <div key={i}>
          <div className={`ln ${e.cls}`}><span className="t">{hhmmss(e.t)}</span><span className="tag">[{e.tag}]</span><span>{e.text}</span></div>
          {e.detail ? <span className="d">{'           '}{e.detail}</span> : null}
        </div>
      ))}
    </div>
  );
}
