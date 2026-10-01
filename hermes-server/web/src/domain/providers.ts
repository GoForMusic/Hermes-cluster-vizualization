// What the UI knows about each kind of source. Pure data: adding a provider is adding an entry here (and its icon).
import type { ProviderId } from './model';

export interface ProviderInfo {
  label: string;
  short: string;
  icon: string;
  color: string;
  help: string;
}

export const PROVIDERS: Record<ProviderId, ProviderInfo> = {
  kubernetes: { label: 'Kubernetes', short: 'K8s', icon: 'kubernetes', color: '#6f9bff', help: 'Kubernetes cluster — orchestrator that runs pods on several machines.' },
  swarm: { label: 'Docker Swarm', short: 'Swarm', icon: 'docker', color: '#35c9ec', help: 'Docker Swarm cluster — runs Docker containers (tasks) on several machines.' },
  nomad: { label: 'Nomad', short: 'Nomad', icon: 'nomad', color: '#c58bff', help: 'HashiCorp Nomad cluster — schedules jobs (allocations) on several machines.' },
  storage: { label: 'Network storage', short: 'NAS', icon: 'nas', color: '#a3b0c4', help: 'NAS — a storage box on the network. Clusters mount its folders (NFS) for media, backups, etc.' },
};

export const TF_COLOR = '#9b8cff';

export const KIND_HELP = {
  host: 'A machine (VM or PC). Control-plane / manager / server hosts decide what runs where; workers run the apps.',
  nas: 'A storage box on the network. Apps from every cluster keep shared files here (media, backups) over NFS.',
  workload: 'A running app: pod (Kubernetes), task (Swarm) or allocation (Nomad).',
  volume: 'A disk used by an app or shared over the network. The % is how full it is.',
  network: 'A way to reach apps: a Docker network, or in Kubernetes a Service, an Ingress or a load balancer. Its lane has a coloured line that branches to what is on it or behind it.',
} as const;
