// The log of a container, in a panel at the bottom of the page (like the devtools of a browser): it stays open while the map is used above it.
// The lines are streamed from the agent through the hub as they are read; nothing is kept, and closing the panel stops the reading.
import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useClient } from '../../state/context';
import { Icon } from '../../ui/icons';

export interface LogTarget {
  source: string;
  /** The namespace (or stack); `-` when the workload has none. */
  namespace: string;
  name: string;
  /** Swarm: the exact task. */
  task?: string;
  /** The containers of the pod to choose from; empty when there is only one thing to read. */
  containers: string[];
}

type Status = 'connecting' | 'live' | 'ended' | 'failed';

/** The most lines kept in the page, and shown at once. The oldest go first. */
const KEEP = 20_000;
const SHOWN = 5_000;
const TAILS = [100, 200, 500, 1000, 5000];

const levelOf = (line: string): string => {
  if (/\b(ERROR|FATAL|PANIC|CRITICAL|Exception)\b/i.test(line)) return 'lg-err';
  if (/\bWARN(ING)?\b/i.test(line)) return 'lg-warn';
  return '';
};

export function LogDock({ target, onClose }: { target: LogTarget; onClose: () => void }) {
  const client = useClient();
  const [container, setContainer] = useState(target.containers[0] ?? '');
  const [tail, setTail] = useState(200);
  const [follow, setFollow] = useState(true);
  const [previous, setPrevious] = useState(false);
  const [timestamps, setTimestamps] = useState(false);
  const [wrap, setWrap] = useState(false);
  const [query, setQuery] = useState('');
  const [lines, setLines] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>('connecting');
  const [error, setError] = useState('');
  const [height, setHeight] = useState(300);
  const body = useRef<HTMLDivElement>(null);
  const stick = useRef(true); // the person is at the bottom: new lines scroll into view; scrolling up lets go

  // another workload: start again from its first container
  useEffect(() => { setContainer(target.containers[0] ?? ''); setPrevious(false); }, [target.source, target.namespace, target.name, target.containers]);

  useEffect(() => {
    setLines([]);
    setError('');
    setStatus('connecting');
    stick.current = true;
    const stop = client.logs.open(target.source, target.namespace, target.name, { container, tail, follow, previous, timestamps, task: target.task }, {
      onLines: (batch) => { setStatus('live'); setLines((cur) => { const next = cur.concat(batch); return next.length > KEEP ? next.slice(next.length - KEEP) : next; }); },
      onEnd: () => setStatus('ended'),
      onError: (message) => { setError(message); setStatus('failed'); },
    });
    return stop;
  }, [client, target.source, target.namespace, target.name, target.task, container, tail, follow, previous, timestamps]);

  const text = query.trim().toLowerCase();
  const matching = useMemo(() => (text ? lines.filter((l) => l.toLowerCase().includes(text)) : lines), [lines, text]);
  const shown = matching.length > SHOWN ? matching.slice(matching.length - SHOWN) : matching;

  useLayoutEffect(() => {
    const el = body.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [shown]);

  const onScroll = useCallback(() => {
    const el = body.current;
    if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }, []);

  const drag = (e: React.MouseEvent) => {
    e.preventDefault();
    const move = (m: MouseEvent) => setHeight(Math.max(140, Math.min(window.innerHeight * 0.8, window.innerHeight - m.clientY)));
    const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  const download = () => {
    const blob = new Blob([`${lines.join('\n')}\n`], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `${target.name}${container ? `-${container}` : ''}${previous ? '-previous' : ''}.log`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const word = status === 'connecting' ? 'Connecting…' : status === 'live' ? (follow ? 'Live' : 'Reading…') : status === 'ended' ? 'Ended' : 'Failed';
  return (
    <section className="logdock" style={{ height }} aria-label={`Log of ${target.name}`}>
      <div className="logdock-grip" onMouseDown={drag} role="separator" aria-orientation="horizontal" aria-label="Resize the log" />
      <div className="logdock-bar">
        <span className="who"><Icon name="cube" size={15} /><b>{target.name}</b><span className="muted">{target.namespace === '-' ? '' : target.namespace}</span></span>
        {target.containers.length > 1 ? (
          <select aria-label="Container" value={container} onChange={(e) => setContainer(e.target.value)}>
            {target.containers.map((c) => <option key={c}>{c}</option>)}
          </select>
        ) : null}
        <button className={`pill${previous ? ' on' : ''}`} aria-pressed={previous} title="The container before it was restarted (what it said before it crashed)" onClick={() => setPrevious(!previous)}>Previous</button>
        <button className={`pill${follow ? ' on' : ''}`} aria-pressed={follow} title="Show new lines as they are written" onClick={() => setFollow(!follow)}>Follow</button>
        <button className={`pill${timestamps ? ' on' : ''}`} aria-pressed={timestamps} onClick={() => setTimestamps(!timestamps)}>Timestamps</button>
        <button className={`pill${wrap ? ' on' : ''}`} aria-pressed={wrap} onClick={() => setWrap(!wrap)}>Wrap</button>
        <select aria-label="Last lines" value={tail} onChange={(e) => setTail(Number(e.target.value))}>
          {TAILS.map((t) => <option key={t} value={t}>last {t}</option>)}
        </select>
        <input type="search" aria-label="Search the log" placeholder="Search…" value={query} onChange={(e) => setQuery(e.target.value)} />
        <span className="muted logdock-count">{text ? `${matching.length} of ${lines.length}` : `${lines.length} lines`}</span>
        <span className={`logdock-status ${status}`} role="status">{word}</span>
        <span style={{ flex: 1 }} />
        <button className="btn xs" disabled={!lines.length} onClick={() => setLines([])}>Clear</button>
        <button className="btn xs" disabled={!lines.length} onClick={download}>Download</button>
        <button className="btn xs" aria-label="Close the log" onClick={onClose}>✕</button>
      </div>
      <div className={`logdock-body${wrap ? ' wrap' : ''}`} ref={body} onScroll={onScroll} tabIndex={0}>
        {status === 'failed' ? <div className="logdock-error" role="alert">{error}</div> : null}
        {matching.length > SHOWN ? <div className="muted">… {matching.length - SHOWN} earlier lines are not shown (narrow it with the search, or Download)</div> : null}
        {shown.map((l, i) => (
          <div key={i} className={`logline ${levelOf(l)}`}>{text ? <Marked line={l} text={text} /> : l || ' '}</div>
        ))}
        {status === 'ended' && !lines.length ? <div className="muted">The log is empty.</div> : null}
      </div>
    </section>
  );
}

/** The line with what was searched for marked. */
function Marked({ line, text }: { line: string; text: string }) {
  const lower = line.toLowerCase();
  const parts: React.ReactNode[] = [];
  let at = 0;
  for (let i = lower.indexOf(text); i >= 0 && parts.length < 200; i = lower.indexOf(text, at)) {
    parts.push(<Fragment key={i}>{line.slice(at, i)}<mark>{line.slice(i, i + text.length)}</mark></Fragment>);
    at = i + text.length;
  }
  parts.push(line.slice(at));
  return <>{parts}</>;
}
