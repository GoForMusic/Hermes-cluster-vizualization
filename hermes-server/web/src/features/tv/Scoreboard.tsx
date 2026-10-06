// The sidebar's per-cluster host table: CPU, memory, workload count and a 24h heartbeat bar for each host.
import { useMemo } from 'react';
import type { HubState } from '../../domain/hubState';
import type { Node } from '../../domain/model';
import { hostsOf, shownKids, summary, visibleClusters, type Summary } from '../../domain/selectors';
import { isRunning } from '../../domain/status';
import { useHubState, useWholeState } from '../../state/context';
import { HeartbeatBar } from '../../ui/status';

const TEAM = { kubernetes: 'k', swarm: 's', docker: 's', nomad: 'n', storage: 'd' } as const;

const summaryOf = (s: HubState): Summary => summary(s);
const sameSummary = (a: Summary, b: Summary) => JSON.stringify(a) === JSON.stringify(b);

export function Scoreboard() {
  const state = useWholeState();
  const s = useHubState(summaryOf, sameSummary);
  // Walking every cluster's hosts and workloads only needs to happen when what it draws from actually changes (the nodes, the parent
  // map, the settings that decide who's shown, or the uptime bars) — not on every hub event, e.g. an unrelated alert ack.
  const rows = useMemo(
    () => visibleClusters(state).map((c) => {
      const hosts = hostsOf(state, c.id);
      return [
        <tr key={c.id} className={`team ${TEAM[c.provider]}`}>
          <td colSpan={2}>{c.name}</td>
          <td className="r" colSpan={4}>{hosts.filter((h) => isRunning(h.status)).length}/{hosts.length}</td>
        </tr>,
        ...hosts.map((h) => <HostRow key={h.id} host={h} workloads={shownKids(state, h.id).filter((n) => n.kind === 'workload')} bars={state.uptime.get(h.id)?.bars ?? []} />),
      ];
    }),
    [state.nodes, state.kids, state.settings, state.uptime],
  );
  return (
    <section className="panel">
      <div className="panel-h">Hosts<em>{s.hostsUp}/{s.hostsTotal} up</em></div>
      <div className="panel-b" style={{ padding: 0 }}>
        <table className="score">
          <thead><tr><th /><th>Host</th><th className="r">CPU</th><th className="r">MEM</th><th className="r">WL</th><th>24h</th></tr></thead>
          <tbody>{rows}</tbody>
        </table>
      </div>
    </section>
  );
}

function HostRow({ host, workloads, bars }: { host: Node; workloads: Node[]; bars: readonly string[] }) {
  const running = workloads.filter((n) => isRunning(n.status)).length;
  const down = host.status === 'crit' || host.status === 'unknown'; // no reading to show
  return (
    <tr className={`host st-${host.status}${down ? ' crit' : ''}`}>
      <td className="st"><i /></td>
      <td>{host.name}</td>
      <td className="r">{down ? '—' : `${Math.round(host.m.cpu ?? 0)}%`}</td>
      <td className="r">{down ? '—' : `${Math.round(host.m.mem ?? 0)}%`}</td>
      <td className="r">{workloads.length ? `${running}/${workloads.length}` : '—'}</td>
      <td className="bars"><HeartbeatBar bars={bars} small take={16} /></td>
    </tr>
  );
}
