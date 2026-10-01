// What the web app works with. The wire types are generated from the Rust model (`generated/`); these are the same things with the
// closed sets narrowed (a node's kind is one of four words, not any string) and the free-form `meta` and `m` given the keys agents fill in.
import type { Alert } from '../generated/Alert';
import type { Edge as WireEdge } from '../generated/Edge';
import type { Node as WireNode } from '../generated/Node';
import type { Uptime } from '../generated/Uptime';

export type { Alert, Uptime };

export type Own = 'ok' | 'warn' | 'crit';
/** What the web app shows: the state the source reports, corrected by what is known about the host and the source. */
export type Status = Own | 'unknown';
export type Kind = 'cluster' | 'host' | 'workload' | 'volume' | 'network';
export type ProviderId = 'kubernetes' | 'swarm' | 'nomad' | 'storage';
/** `route`: a path with no measured rate (a network and who is on it, an Ingress and its Services). */
export type EdgeType = 'traffic' | 'control' | 'route';

export interface ContainerInfo {
  name: string;
  image: string;
  ready: boolean;
  state: string;
  restarts: number;
}

export interface IacInfo {
  ref: string;
  drift: boolean;
  note: string;
}

/** The facts agents attach to a node. Free-form on the wire: everything is optional. */
export interface NodeMeta {
  version?: string;
  api?: string;
  uid?: string;
  ip?: string;
  role?: string;
  vcpu?: number;
  ram?: number;
  os?: string;
  osType?: string;
  arch?: string;
  iac?: IacInfo;
  type?: string;
  ns?: string;
  image?: string;
  restarts?: number;
  containers?: ContainerInfo[];
  size?: number;
  sc?: string;
  mountedBy?: string;
  usageKnown?: boolean;
  /** Of a network: what it is (overlay, macvlan, service, ingress, loadbalancer...), its address range and how many workloads are on it. */
  netKind?: string;
  subnet?: string;
  /** Of a Kubernetes Service, Ingress or load balancer: where it is reached (cluster IP and ports, host names, an external address). */
  addr?: string;
  /** A load balancer that has not been given an address yet. */
  pending?: boolean;
  internal?: boolean;
  encrypted?: boolean;
  members?: number;
}

/** The numbers agents measure. Hosts report cpu and mem as percentages; pods report absolute usage. */
export interface NodeMetrics {
  cpu?: number;
  mem?: number;
  cpuMilli?: number;
  memMiB?: number;
  rxMbps?: number;
  txMbps?: number;
  used?: number;
}

export interface Node {
  id: string;
  kind: Kind;
  name: string;
  parent: string | null;
  provider: ProviderId;
  own: Own;
  status: Status;
  reason: string;
  since: number;
  m: NodeMetrics;
  meta: NodeMeta;
  /** The source that reports this node is unreachable: `own` and `m` are the last known values, not current ones. */
  stale: boolean;
}

export interface Edge {
  id: string;
  from: string;
  to: string;
  base: number;
  mbps: number;
  type: EdgeType;
}

const KINDS: readonly Kind[] = ['cluster', 'host', 'workload', 'volume', 'network'];
const EDGE_TYPES: readonly EdgeType[] = ['traffic', 'control', 'route'];
const PROVIDERS: readonly ProviderId[] = ['kubernetes', 'swarm', 'nomad', 'storage'];
const OWNS: readonly Own[] = ['ok', 'warn', 'crit'];

function oneOf<T extends string>(allowed: readonly T[], value: string, fallback: T): T {
  return (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
}

/** A node as the hub sends it, in the shape the app uses. A value the app does not know becomes the most neutral one. */
export function parseNode(wire: WireNode): Node {
  const own = oneOf(OWNS, wire.own, 'ok');
  return {
    id: wire.id,
    kind: oneOf(KINDS, wire.kind, 'workload'),
    name: wire.name,
    parent: wire.parent,
    provider: oneOf(PROVIDERS, wire.provider, 'kubernetes'),
    own,
    status: own,
    reason: wire.reason,
    since: wire.since,
    m: wire.m,
    meta: wire.meta as NodeMeta,
    stale: wire.stale ?? false,
  };
}

export function parseEdge(wire: WireEdge): Edge {
  return { ...wire, type: oneOf(EDGE_TYPES, wire.type, 'traffic') };
}
