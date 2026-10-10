// The description of a node: the same rows on the dashboard and beside the map.
import type { ReactNode } from 'react';
import { fmtCpu, fmtDur, fmtMbps, fmtMem, fmtSize } from '../../domain/format';
import type { HubState } from '../../domain/hubState';
import type { Node } from '../../domain/model';
import { isGateway } from '../../domain/mapLabels';
import { KIND_HELP, PROVIDERS } from '../../domain/providers';
import { clusterOf, getNode, isSystem, kidsOf } from '../../domain/selectors';
import { STATUS_WORD } from '../../domain/status';
import { useNow } from '../../ui/hooks';
import { StatusChip } from '../../ui/status';

export function helpFor(n: Node): string {
  if (n.kind === 'cluster') return PROVIDERS[n.provider].help;
  if (n.kind === 'host') return n.meta.role === 'storage' ? KIND_HELP.nas : KIND_HELP.host;
  return KIND_HELP[n.kind];
}

const Mono = ({ children }: { children: ReactNode }) => <span className="mono">{children}</span>;

export function NodeDetails({ state, node: n }: { state: HubState; node: Node }) {
  const now = useNow(1000);
  const cl = clusterOf(state, n.id);
  const host = n.parent ? getNode(state, n.parent) : undefined;
  const rows: [string, ReactNode][] = [];
  const kv = (k: string, v: ReactNode) => rows.push([k, v]);

  kv('Kind', n.kind === 'workload' || n.kind === 'network' ? n.meta.type : n.kind);
  if (cl) kv('Cluster', `${cl.name} (${PROVIDERS[cl.provider].label})`);
  kv('Status', <StatusChip status={n.status}>{n.reason ? `${STATUS_WORD[n.status]} · ${n.reason}` : STATUS_WORD[n.status]}</StatusChip>);
  kv('For', fmtDur(now - n.since));

  if (n.kind === 'cluster') {
    kv('Version', n.meta.version);
    kv('API', <Mono>{n.meta.api}</Mono>);
  } else if (n.kind === 'host') {
    kv('IP', <Mono>{n.meta.ip}</Mono>);
    kv('Role', n.meta.role);
    if (n.meta.location) kv('Location', n.meta.location);
    kv('Resources', `${n.meta.vcpu} vCPU · ${Number(n.meta.ram).toFixed(1)} GiB RAM`);
    kv('OS', `${n.meta.osType ?? '?'}${n.meta.arch ? `/${n.meta.arch}` : ''} · ${n.meta.os}`);
    kv('Load', n.m.cpu == null ? '—' : `CPU ${n.m.cpu.toFixed(1)}% · memory ${(n.m.mem ?? 0).toFixed(1)}%`);
    const net = kidsOf(state, n.id).filter((k) => k.kind === 'workload' && k.m.rxMbps != null); // what its pods and tasks move, added up
    if (net.length) kv('Network', `↓ ${fmtMbps(net.reduce((s, k) => s + (k.m.rxMbps ?? 0), 0))} received · ↑ ${fmtMbps(net.reduce((s, k) => s + (k.m.txMbps ?? 0), 0))} sent (pods)`);
    if (n.meta.iac) kv('Terraform', n.meta.iac.drift ? `drift — ${n.meta.iac.note}` : `managed (${n.meta.iac.ref})`);
  } else if (n.kind === 'workload') {
    kv('Image', <Mono>{n.meta.image}</Mono>);
    kv(cl?.provider === 'swarm' ? 'Stack' : 'Namespace', n.meta.ns);
    if (isSystem(state, n)) kv('Scope', 'System: part of the cluster itself, not one of your apps');
    if (host) kv('Host', host.name);
    kv('Restarts', n.meta.restarts ?? 0);
    const pct = (v: number | undefined) => (v == null ? '' : ` (${v.toFixed(1)}% of node)`);
    kv('CPU', n.m.cpuMilli == null ? '—' : `${fmtCpu(n.m.cpuMilli)}${pct(n.m.cpu)}`);
    kv('Memory', n.m.memMiB == null ? '—' : `${fmtMem(n.m.memMiB)}${pct(n.m.mem)}`);
    kv('Network', n.m.rxMbps == null ? '—' : `↓ ${fmtMbps(n.m.rxMbps)} received · ↑ ${fmtMbps(n.m.txMbps ?? 0)} sent`);
  } else if (n.kind === 'network') {
    kv(isGateway(n) ? 'Kind' : 'Driver', n.meta.netKind);
    if (n.meta.addr) kv('Address', <Mono>{n.meta.addr}</Mono>);
    if (n.meta.pending) kv('Note', 'Waiting for an address: nothing in the cluster hands one out (a load balancer such as MetalLB is needed)');
    if (n.meta.subnet) kv('Range', <Mono>{n.meta.subnet}</Mono>);
    if (n.meta.ns) kv(cl?.provider === 'swarm' ? 'Stack' : 'Namespace', n.meta.ns);
    if (isGateway(n)) kv(n.meta.netKind === 'policy' ? 'Applies to' : 'Leads to', `${n.meta.members ?? 0} ${['outside', 'ingress', 'gateway', 'httproute'].includes(n.meta.netKind ?? '') ? 'entries' : 'pods'}`);
    else kv('On it', `${n.meta.members ?? 0} ${n.meta.members === 1 ? 'workload' : 'workloads'}`);
    if (n.meta.internal) kv('Scope', 'Internal: no route to the outside');
    if (n.meta.encrypted) kv('Traffic', 'Encrypted between nodes');
  } else {
    const used = n.m.used ?? 0;
    const size = n.meta.size ?? 0;
    kv('Size', size > 0 ? fmtSize(size) : 'no limit set');
    kv('Used', n.meta.usageKnown === false ? 'unknown (not measured yet)' : size > 0 ? `${fmtSize(used)} (${Math.round((used / size) * 100)}%)` : fmtSize(used));
    kv('Class', n.meta.sc || '—');
    kv('Mounted by', n.meta.mountedBy);
    if (host) kv('Host', host.name);
  }
  return (
    <dl className="kv">
      {rows.flatMap(([k, v]) => [<dt key={`k${k}`}>{k}</dt>, <dd key={`v${k}`}>{v}</dd>])}
    </dl>
  );
}

export function ContainerList({ node }: { node: Node }) {
  const list = node.meta.containers;
  if (!list?.length) return null;
  return (
    <div className="sec">
      <h4>Containers</h4>
      <div className="list">
        {list.map((c) => (
          <div key={c.name} className="link-row">
            <span>{c.name}<span className="muted">{'  '}{c.image}</span></span>
            <span className={c.ready ? '' : 'muted'}>{c.state}{c.restarts ? ` · ${c.restarts}↻` : ''}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
