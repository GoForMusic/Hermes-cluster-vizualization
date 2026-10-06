// The collapsible "how to read this" panel, positioned by the container it is in.
import type { ReactNode } from 'react';
import { KIND_HELP, PROVIDERS, TF_COLOR } from '../../domain/providers';
import { Icon } from '../../ui/icons';
import { STATUS_FILL, StatusChip } from '../../ui/status';

function Symbol({ kind, label }: { kind: keyof typeof STATUS_FILL; label: string }) {
  return (
    <div>
      <svg viewBox="0 0 44 32" width={44} height={32} aria-hidden="true">
        {kind === 'crit'
          ? <polygon points="22,2 41,16 22,30 3,16" fill={STATUS_FILL.crit} stroke="#05090e" strokeWidth={2} />
          : <rect x={6} y={5} width={32} height={22} fill={STATUS_FILL[kind]} stroke={kind === 'unknown' ? '#7d8a9b' : '#05090e'} strokeWidth={2} strokeDasharray={kind === 'unknown' || kind === 'system' ? '4 3' : undefined} />}
      </svg>
      {label}
    </div>
  );
}

function LinkSample({ kind }: { kind: 'traffic' | 'control' | 'route' | 'broken' }) {
  return (
    <svg className="lg-link" viewBox="0 0 44 16" width={44} height={16} aria-hidden="true">
      <path d="M2 8H42" fill="none" stroke={kind === 'broken' ? 'var(--crit)' : kind === 'traffic' ? 'var(--edge)' : 'var(--dim)'} strokeWidth={kind === 'traffic' ? 2.2 : kind === 'route' ? 1.2 : 1.6} strokeDasharray={kind === 'control' ? '4 3' : kind === 'route' ? '2 4' : kind === 'broken' ? '3 4' : undefined} opacity={kind === 'broken' ? 0.7 : 1} />
      {kind === 'traffic' ? <><circle cx={16} cy={8} r={3.2} fill="var(--accent)" /><circle cx={32} cy={8} r={3.2} fill="var(--accent)" /></> : null}
    </svg>
  );
}

/** The icons on the hexagons of the map, one for each way in (or rule) a cluster can have. */
export const NETWORK_KINDS = [
  { icon: 'globe', title: 'Outside', help: 'Where traffic comes from: your LAN or the internet. It leads to the Ingresses, Gateways and load balancers.' },
  { icon: 'gate', title: 'Ingress / Gateway', help: 'The door of a Kubernetes cluster: it sends requests for a host name to a Service (Ingress, or Gateway API).' },
  { icon: 'fanout', title: 'Service / route', help: 'A stable address in front of pods (or a Gateway API route): the line goes on to each pod behind it.' },
  { icon: 'balance', title: 'Load balancer', help: 'A Service reachable from outside the cluster. \"no address yet\" = nothing hands it an address (MetalLB, a cloud).' },
  { icon: 'shield', title: 'Network policy', help: 'A firewall rule between pods: it applies to the pods its line reaches (\"deny ingress\" = nothing may come in).' },
] as const;

const Row = ({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) => (
  <div className="lg-row"><div className="lg-ic">{icon}</div><div><b>{title}</b><span>{children}</span></div></div>
);

export function Legend({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <div className={`legend${open ? ' open' : ''}`}>
      <button className="btn xs legend-btn" onClick={() => onOpenChange(!open)}>? Legend</button>
      <div className="lg-body">
        <div className="lg-h"><span>How to read this</span><button className="btn xs ghost" onClick={() => onOpenChange(false)}>✕</button></div>
        <Row icon={<span className="lg-provs"><Icon name="kubernetes" size={17} color={PROVIDERS.kubernetes.color} /><Icon name="docker" size={17} color={PROVIDERS.swarm.color} /><Icon name="nomad" size={17} color={PROVIDERS.nomad.color} /></span>} title="Cluster">A group of machines managed by Kubernetes, Docker Swarm or Nomad. A Docker machine on its own (no swarm) is drawn the same way: a box with that one machine in it.</Row>
        <Row icon={<Icon name="server" size={20} color="var(--muted)" />} title="Host">{KIND_HELP.host}</Row>
        <Row icon={<span className="mono" style={{ fontSize: 10, lineHeight: 1.2, textAlign: 'center' }}>LINUX<br />WINDOWS</span>} title="OS tag">The operating system of a host. Kubernetes, Swarm and Docker can mix Linux and Windows machines; the agent for each is a separate image.</Row>
        <Row icon={<Icon name="cube" size={20} color="var(--muted)" />} title="Workload">{KIND_HELP.workload}</Row>
        <Row icon={<Icon name="disk" size={20} color="var(--muted)" />} title="Volume">{KIND_HELP.volume}</Row>
        <Row icon={<Icon name="net" size={20} color="var(--muted)" />} title="Network">{KIND_HELP.network}</Row>
        {NETWORK_KINDS.map((k) => <Row key={k.title} icon={<Icon name={k.icon} size={20} color="var(--muted)" />} title={k.title}>{k.help}</Row>)}
        <Row icon={<Icon name="nas" size={20} color={PROVIDERS.storage.color} />} title="NAS">{KIND_HELP.nas} That is why lines from different clusters end at it.</Row>
        <Row icon={<LinkSample kind="traffic" />} title="Traffic">Data moving between two things, in Mb/s. The arrow points from the caller to the service it calls; moving dots = data flowing.</Row>
        <Row icon={<LinkSample kind="route" />} title="Route">A path exists. When the flows agent runs on the nodes, the line also says how much data moves on it (Mb/s, moving dots); without it, the line only says there is a path. Each network is a hexagon above the hosts with its own colour: its line runs down to a bus and branches to every app on it. A dotted line joins an app to the volume it mounts.</Row>
        <Row icon={<LinkSample kind="control" />} title="Control traffic">Dashed line under the hosts: the cluster brain talking to its workers (heartbeats, scheduling).</Row>
        <Row icon={<LinkSample kind="broken" />} title="Broken link">One end is down, so no traffic.</Row>
        <Row icon={<span className="mono" style={{ fontSize: 10, lineHeight: 1.2, textAlign: 'center' }}>MOUSE</span>} title="Move the map">Wheel zooms. Drag with the middle button to move the map (the left one too, from the empty background). Double click the background to fit it. Drag a cluster&apos;s title band to rearrange: left or right of another = the same line, above or below = a line of its own (double click a band: automatic). Drag the corner of a host to change its width.</Row>
        <Row icon={<Icon name="terraform" size={20} color={TF_COLOR} />} title="Terraform">“managed” = the machine is defined in Terraform code. “drift” = it was changed by hand and no longer matches the code.</Row>
        <div className="lg-row">
          <div className="lg-ic" />
          <div>
            <b>State symbols</b>
            <span>Cyan = your app is healthy, yellow = warning, red diamond = failing, dashed grey = no data because its host is down. Dashed green = a system pod (the cluster&apos;s own component, like kube-system or calico) that is healthy; when a system pod fails it turns yellow or red like any other.</span>
            <div className="lg-syms"><Symbol kind="ok" label="Healthy" /><Symbol kind="system" label="System" /><Symbol kind="warn" label="Warning" /><Symbol kind="crit" label="Critical" /><Symbol kind="unknown" label="No data" /></div>
          </div>
        </div>
        <div className="lg-status"><StatusChip status="ok" /><StatusChip status="warn" /><StatusChip status="crit" /><StatusChip status="unknown">Unknown · host down</StatusChip></div>
      </div>
    </div>
  );
}
