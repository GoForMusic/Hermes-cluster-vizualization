// Sources: what the hub watches. Adding one generates the manifest that installs an agent inside the cluster; the agent reports out to the hub.
import { useEffect, useState, type ReactElement } from 'react';
import type { AddSourceResponse } from '../../../generated/AddSourceResponse';
import type { SourceView } from '../../../generated/SourceView';
import type { RegistryTest } from '../../../generated/RegistryTest';
import { useClient, useStore, useHubState } from '../../../state/context';
import { Dialog } from '../../../ui/dialog';
import { StatusChip } from '../../../ui/status';
import { errorText, useAsyncSubmit } from '../../../ui/hooks';
import { PageHead } from '../PageHead';

const STATE_CHIP: Record<string, () => ReactElement> = {
  connected: () => <StatusChip status="ok">Connected</StatusChip>,
  pending: () => <StatusChip status="unknown">Connecting</StatusChip>,
  error: () => <StatusChip status="crit">Error</StatusChip>,
  duplicate: () => <StatusChip status="warn">Duplicate</StatusChip>,
};

export function Sources() {
  const sources = useHubState((s) => s.sources);
  const [adding, setAdding] = useState(false);
  const [installed, setInstalled] = useState<AddSourceResponse | null>(null);
  const store = useStore();
  const pending = sources.some((x) => x.upgrade?.state === 'pending');
  // a change that takes minutes is followed here: an agent that is replaced says its new version by itself, but a failure is only found out by asking
  useEffect(() => {
    if (!pending) return;
    const t = setInterval(() => void store.refreshSources(), 5000);
    return () => clearInterval(t);
  }, [pending, store]);
  return (
    <>
      <PageHead title="Sources"><button className="btn primary" onClick={() => setAdding(true)}>+ Add source</button></PageHead>
      <p className="help">A source is a cluster the hub watches. An agent runs inside it and reports out to the hub: the hub holds no cluster credentials, and it works behind NAT.</p>
      <div className="src-grid">
        {sources.length
          ? sources.map((s) => <SourceCard key={s.id} source={s} />)
          : <div className="card" style={{ gridColumn: '1 / -1' }}><div className="empty"><b>No sources yet</b>Add your first cluster to see it on the map and the TV.</div></div>}
      </div>
      <AddDialog open={adding} onClose={() => setAdding(false)} onAdded={(r) => { setAdding(false); setInstalled(r); }} />
      <InstallDialog added={installed} onClose={() => setInstalled(null)} />
    </>
  );
}

function SourceCard({ source: s }: { source: SourceView }) {
  const store = useStore();
  const [removing, setRemoving] = useState(false);
  const [changing, setChanging] = useState(false);
  const [editing, setEditing] = useState(false);
  return (
    <div className="card src-card">
      <div className="src-top"><b>{s.name}</b>{(STATE_CHIP[s.state] ?? STATE_CHIP.pending!)()}</div>
      <div className="muted">{s.type}</div>
      <div className="src-meta">{s.info || '—'}</div>
      <AgentsLine source={s} />
      <div className="src-meta">{s.endpoint || '—'} · {s.auth || '—'}</div>
      <UpgradeLine source={s} />
      {s.builtin ? null : (
        <div className="src-actions">
          <button className="btn xs" onClick={() => setEditing(true)}>Edit</button>
          <button className="btn xs" onClick={() => setChanging(true)}>Change agent version</button>
          <button className="btn xs danger" onClick={() => setRemoving(true)}>Remove</button>
        </div>
      )}
      <RemoveDialog source={s} open={removing} onClose={() => setRemoving(false)} onConfirm={() => void store.removeSource(s.id)} />
      <ChangeVersionDialog source={s} open={changing} onClose={() => setChanging(false)} />
      <EditDialog source={s} open={editing} onClose={() => setEditing(false)} />
    </div>
  );
}

/** Renaming a source: only the name changes. The agents already installed keep working, so there is nothing to deploy again. */
function EditDialog({ source: s, open, onClose }: { source: SourceView; open: boolean; onClose: () => void }) {
  const store = useStore();
  const [name, setName] = useState(s.name);
  useEffect(() => { if (open) setName(s.name); }, [open, s.name]);
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await store.renameSource(s.id, name.trim());
    onClose();
  });
  return (
    <Dialog open={open} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Edit {s.name}</h2>
        <div className="form">
          <label className="f">Name<input type="text" value={name} onChange={(e) => setName(e.target.value)} /></label>
          <div className="help" style={{ margin: 0 }}>Only the name changes. The agent already installed keeps working: there is nothing to deploy again.</div>
          <div className="err">{error}</div>
        </div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !name.trim() || name.trim() === s.name}>Save</button>
        </div>
      </form>
    </Dialog>
  );
}

/** Which version each agent of the source runs (from its hello), and whether the install manifests already ask for a newer one. */
function AgentsLine({ source: s }: { source: SourceView }) {
  const live = s.agents.filter((a) => a.seenAgo <= 30); // an instance that has been replaced lingers in the list for a while
  if (!live.length) return null;
  const versions = [...new Set(live.map((a) => (a.version ? `v${a.version}` : 'unknown version')))];
  const behind = live.filter((a) => a.outdated).length;
  const oldProtocol = live.filter((a) => a.protocolOutdated).length;
  return (
    <div className="src-meta src-agents">
      Agent {versions.join(', ')}{live.length > 1 ? ` · ${live.length} instances` : ''}
      {behind ? <StatusChip status="warn">{behind > 1 ? `${behind} behind` : 'update'} → v{s.expectedAgent}</StatusChip> : null}
      {oldProtocol ? <StatusChip status="warn">old protocol — upgrade the agent</StatusChip> : null}
    </div>
  );
}

/** Removing a source is not undone by anything but adding it again with a new token: ask, the way the other actions of the card do. */
function RemoveDialog({ source: s, open, onClose, onConfirm }: { source: SourceView; open: boolean; onClose: () => void; onConfirm: () => void }) {
  return (
    <Dialog open={open} onClose={onClose}>
      <h2>Remove {s.name}?</h2>
      <p className="help" style={{ marginTop: 0 }}>
        The hub forgets this source, its history and its alerts, and its agents are turned away (their token stops working). Nothing is deleted in the cluster: remove the agent there yourself.
      </p>
      <div className="dlg-actions">
        <button type="button" className="btn" onClick={onClose}>Cancel</button>
        <button type="button" className="btn danger solid" onClick={() => { onClose(); onConfirm(); }}>Remove source</button>
      </div>
    </Dialog>
  );
}

/** How the last version change is going: the cluster replaces the agents one at a time, and the hub sees each new one say its version. */
function UpgradeLine({ source: s }: { source: SourceView }) {
  const u = s.upgrade;
  if (!u) return null;
  return (
    <div className="src-meta src-agents">
      {u.state === 'pending' ? <StatusChip status="unknown">Changing to v{u.version}…</StatusChip> : null}
      {u.state === 'done' ? <StatusChip status="ok">Now v{u.version}</StatusChip> : null}
      {u.state === 'failed' ? <><StatusChip status="crit">Change to v{u.version} failed</StatusChip><span>{u.message}</span></> : null}
    </div>
  );
}

/** The versions in the registry, for one source: pick one to move the agents to it, newer (update) or older (roll back). */
function ChangeVersionDialog({ source: s, open, onClose }: { source: SourceView; open: boolean; onClose: () => void }) {
  const store = useStore();
  const client = useClient();
  const [found, setFound] = useState<RegistryTest | null>(null);
  const [chosen, setChosen] = useState('');
  const running = [...new Set(s.agents.filter((a) => a.seenAgo <= 30 && a.version).map((a) => a.version))];

  useEffect(() => {
    if (!open) return;
    let live = true;
    setChosen('');
    void client.registry.versions().then((r) => { if (live) setFound(r); }).catch((e: unknown) => { if (live) setFound({ ok: false, message: errorText(e), versions: [] }); });
    return () => { live = false; };
  }, [client, open]);

  const { busy, error, submit } = useAsyncSubmit(
    async () => { await store.upgradeSource(s.id, chosen); onClose(); },
    () => (chosen ? null : 'Choose a version.'),
  );
  const versions = found?.ok ? found.versions.filter((v) => /^\d+\.\d+\.\d+/.test(v)) : [];

  return (
    <Dialog open={open} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Change agent version · {s.name}</h2>
        {running.length ? <p className="help" style={{ marginTop: 0 }}>Running now: {running.map((v) => `v${v}`).join(', ')}.</p> : null}
        {!s.canUpgrade ? (
          <div className="err" role="status">
            The agents of this source were not installed to change their own version. Add the source again with “Allow upgrades from the dashboard” ticked, and apply that manifest once: from then on the version can be changed here.
          </div>
        ) : null}
        {found && !found.ok ? <div className="err" role="alert">{found.message}</div> : null}
        {found?.ok && !versions.length ? <p className="help">{found.message}. Set up the registry in Settings, and push the agent image there.</p> : null}
        <div className="ver-list" role="radiogroup" aria-label="Agent versions">
          {versions.map((v, i) => (
            <label key={v} className={`ver-row${chosen === v ? ' on' : ''}`}>
              <input type="radio" name="version" value={v} checked={chosen === v} onChange={() => setChosen(v)} disabled={!s.canUpgrade} />
              <span>v{v}</span>
              <span className="muted">{[i === 0 ? 'newest' : '', running.includes(v) ? 'running now' : ''].filter(Boolean).join(' · ')}</span>
            </label>
          ))}
        </div>
        <p className="help">
          Each agent changes its own image and keeps its registry: Kubernetes replaces the pods one at a time, Swarm replaces the tasks starting the new one before stopping the old. A version that does not come up is put back.
        </p>
        <div className="err">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !s.canUpgrade}>Apply</button>
        </div>
      </form>
    </Dialog>
  );
}

function AddDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded: (r: AddSourceResponse) => void }) {
  const store = useStore();
  const types = useHubState((s) => s.info.sourceTypes);
  const [name, setName] = useState('');
  const [type, setType] = useState('');
  const [hubUrl, setHubUrl] = useState(location.origin);
  const [flows, setFlows] = useState(false);
  const [windows, setWindows] = useState(false);
  const [upgrades, setUpgrades] = useState(true);
  const [version, setVersion] = useState('');
  const registry = useRegistryVersions(open);
  const chosen = type || types[0] || '';
  const kubernetes = chosen.startsWith('Kubernetes');
  const docker = chosen === 'Docker (agent)'; // machines of their own: no swarm, no upgrade from the dashboard yet

  const { busy, error, submit } = useAsyncSubmit(async () => {
    if (registry.configured && !registry.implicit && !(version || registry.versions[0])) throw new Error('The registry has no agent version to install yet: push the agent image there first.');
    const added = await store.addSource({
      name, type: chosen, endpoint: '', hubUrl, flows: kubernetes && flows, upgrades: upgrades && !docker,
      version: registry.configured ? version || registry.versions[0] || '' : '', windows: registry.configured && !kubernetes && windows,
    });
    setName('');
    onAdded(added);
  });

  return (
    <Dialog open={open} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Add source</h2>
        <div className="form">
          <label className="f">Name<input type="text" placeholder="e.g. lab-k3s" value={name} onChange={(e) => setName(e.target.value)} /></label>
          <label className="f">Type<select value={chosen} onChange={(e) => setType(e.target.value)}>{types.map((t) => <option key={t} value={t}>{t}</option>)}</select></label>
          <label className="f">Hub address as seen from the agent<input type="text" value={hubUrl} onChange={(e) => setHubUrl(e.target.value)} /></label>
          {registry.configured ? (
            <>
              <label className="f">Agent version
                <select value={version || registry.versions[0] || ''} onChange={(e) => setVersion(e.target.value)} disabled={!registry.versions.length}>
                  {registry.versions.length ? registry.versions.map((v, i) => <option key={v} value={v}>{v}{i === 0 ? ' (newest)' : ''}</option>) : <option value="">none found in the registry</option>}
                </select>
                <span className="muted" style={{ fontSize: 11.5 }}>{registry.message}{registry.implicit && !registry.versions.length ? ' Without a version the file uses the image this hub was started with.' : ''}</span>
              </label>
              {kubernetes ? null : (
                <label className="check">
                  <input type="checkbox" checked={windows} onChange={(e) => setWindows(e.target.checked)} />
                  <span><b>{docker ? 'Some machines run Windows' : 'The cluster has Windows nodes'}</b><span className="muted">Adds the Windows agent to the {docker ? 'compose file' : 'stack'} ({version || registry.versions[0] || 'version'}-ltsc2022).</span></span>
                </label>
              )}
            </>
          ) : <div className="help" style={{ margin: 0 }}>No registry is set up: the manifest uses the image the hub was started with. Set one up in Settings → Registry to choose versions.</div>}
          <div className="help" style={{ margin: 0 }}>The agent connects OUT to this address (gRPC, the same port as this page). It must not be localhost.</div>
          {docker ? null : <label className="check">
            <input type="checkbox" checked={upgrades} onChange={(e) => setUpgrades(e.target.checked)} />
            <span>
              <b>Allow upgrades from the dashboard</b>
              <span className="muted">
                Lets the agent change its own image when you pick a version (Change agent version). {kubernetes ? 'It gets the right to read and patch only its own Deployment and DaemonSet.' : 'It uses the Docker socket it already has.'} Untick to keep it strictly read-only: you then update it by applying a new manifest.
              </span>
            </span>
          </label>}
          {kubernetes ? (
            <label className="check">
              <input type="checkbox" checked={flows} onChange={(e) => setFlows(e.target.checked)} />
              <span>
                <b>Show who talks to whom</b>
                <span className="muted">
                  The agent on each node also reads the kernel&apos;s connection table, and the map shows real Mb/s on the links from a Service to its pods and from outside. The node agents then run in the node&apos;s
                  network (hostNetwork), still as root with no capabilities (no NET_ADMIN). The node needs <code>net.netfilter.nf_conntrack_acct=1</code>.
                </span>
              </span>
            </label>
          ) : null}
          <div className="err">{error}</div>
        </div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>Add</button>
        </div>
      </form>
    </Dialog>
  );
}

/** Whether a registry is set up, and the agent versions in it: what the Add dialog offers. Looked up each time the dialog opens. */
function useRegistryVersions(open: boolean): { configured: boolean; implicit: boolean; versions: string[]; message: string } {
  const client = useClient();
  const [state, setState] = useState({ configured: false, implicit: false, versions: [] as string[], message: '' });
  useEffect(() => {
    if (!open) return;
    let live = true;
    void (async () => {
      try {
        const saved = await client.registry.get();
        if (!saved.url) { if (live) setState({ configured: false, implicit: false, versions: [], message: '' }); return; }
        const found: RegistryTest = await client.registry.versions().catch((e: unknown) => ({ ok: false, message: e instanceof Error ? e.message : String(e), versions: [] }));
        if (live) setState({ configured: true, implicit: saved.implicit, versions: found.ok ? found.versions : [], message: found.message });
      } catch { if (live) setState({ configured: false, implicit: false, versions: [], message: '' }); }
    })();
    return () => { live = false; };
  }, [client, open]);
  return state;
}

/** Shown once after creating a source: the manifest holds the token. */
function InstallDialog({ added, onClose }: { added: AddSourceResponse | null; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <Dialog open={added != null} onClose={() => { setCopied(false); onClose(); }} wide>
      <h2>Install the agent{added ? ` for “${added.name}”` : ''}</h2>
      <p className="help">This installs the agent next to what it watches. The token is shown only now.</p>
      <pre className="code">{added?.install}</pre>
      <p className="help" style={{ marginTop: 10 }}>{added?.hint}</p>
      <div className="dlg-actions">
        <button className="btn" onClick={() => { void navigator.clipboard?.writeText(added?.install ?? ''); setCopied(true); }}>{copied ? 'Copied' : 'Copy manifest'}</button>
        <button className="btn primary" onClick={() => { setCopied(false); onClose(); }}>Done</button>
      </div>
    </Dialog>
  );
}
