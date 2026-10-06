// The boundary to the hub, as small interfaces. The store and the screens depend on THESE, never on `fetch`: a test hands them a fake, and the
// transport (HTTP + Server-Sent Events today) can change without touching them. Each interface is what one part of the app needs.
import type { AddSourceRequest } from '../generated/AddSourceRequest';
import type { AddSourceResponse } from '../generated/AddSourceResponse';
import type { AuthStatus } from '../generated/AuthStatus';
import type { Edge as WireEdge } from '../generated/Edge';
import type { HubEvent } from '../generated/HubEvent';
import type { HubInfo } from '../generated/HubInfo';
import type { Node as WireNode } from '../generated/Node';
import type { CommLogEntry } from '../generated/CommLogEntry';
import type { CommLogView } from '../generated/CommLogView';
import type { RegistryInput } from '../generated/RegistryInput';
import type { RegistryTest } from '../generated/RegistryTest';
import type { RegistryView } from '../generated/RegistryView';
import type { SourceView } from '../generated/SourceView';
import type { Alert, Uptime } from '../domain/model';
import type { Settings } from '../domain/settings';

export interface Credentials {
  username: string;
  password: string;
}

export interface IAuthApi {
  status(): Promise<AuthStatus>;
  /** The first visit creates the admin. */
  setup(credentials: Credentials & { publicView: boolean }): Promise<void>;
  login(credentials: Credentials): Promise<void>;
  logout(): Promise<void>;
  changePassword(current: string, next: string): Promise<void>;
  setPublicView(enabled: boolean): Promise<void>;
}

export interface ITopologyApi {
  snapshot(): Promise<{ nodes: WireNode[]; edges: WireEdge[] }>;
  info(): Promise<HubInfo>;
}

export interface IAlertsApi {
  list(limit?: number): Promise<Alert[]>;
  ack(id: number): Promise<void>;
}

export interface ISettingsApi {
  /** What the hub stored: possibly nothing, possibly from an older version. */
  load(): Promise<unknown>;
  save(settings: Settings): Promise<void>;
}

export interface ISourcesApi {
  list(): Promise<SourceView[]>;
  add(request: AddSourceRequest): Promise<AddSourceResponse>;
  /** Changes only the name: the agents already installed keep working and nothing is redeployed. */
  rename(id: string, name: string): Promise<void>;
  remove(id: string): Promise<void>;
  /** Ask the source's agents to run this version; each changes its own image. Progress shows in the source's `upgrade`. */
  upgrade(id: string, version: string): Promise<void>;
}

export interface IRegistryApi {
  get(): Promise<RegistryView>;
  save(input: RegistryInput): Promise<RegistryView>;
  /** Tries the settings as typed (not yet saved) and lists the agent versions the registry holds. */
  test(input: RegistryInput): Promise<RegistryTest>;
  /** The versions in the saved registry. */
  versions(): Promise<RegistryTest>;
}

export interface CommLogQuery {
  source?: string;
  kind?: string;
  q?: string;
  limit?: number;
}

/** The agent ↔ hub communication log: off until an admin turns it on, kept in memory, never holds a token. */
export interface ICommLogApi {
  list(query?: CommLogQuery): Promise<CommLogView>;
  get(id: number): Promise<CommLogEntry>;
  enable(on: boolean): Promise<void>;
  clear(): Promise<void>;
}

export interface IUptimeApi {
  /** `span` in seconds. */
  load(span: number, buckets: number): Promise<Record<string, Uptime>>;
}

export interface FeedHandlers {
  onEvent(event: HubEvent): void;
  onOpen(): void;
  /** `refused`: the hub turned the stream down (not logged in any more), which is not the same as a dropped connection. */
  onError(refused: boolean): void;
}

/** The live events of the hub. Reconnecting is the feed's job; every connection starts with a snapshot. */
export interface IEventFeed {
  connect(handlers: FeedHandlers): () => void;
}

export interface IHubClient {
  auth: IAuthApi;
  topology: ITopologyApi;
  alerts: IAlertsApi;
  settings: ISettingsApi;
  sources: ISourcesApi;
  registry: IRegistryApi;
  commLog: ICommLogApi;
  uptime: IUptimeApi;
  feed: IEventFeed;
}

/** The hub answered with an error. `authRequired`: the session is gone (or was never there), so the person has to log in. */
export class HubError extends Error {
  constructor(message: string, readonly status: number, readonly authRequired = false) {
    super(message);
    this.name = 'HubError';
  }
}
