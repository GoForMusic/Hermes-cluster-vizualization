// Hub-wide settings, one panel per section side by side (TV display, Registry); a narrow screen stacks them. Saved on the hub, so every
// open TV/admin session picks changes up live.
import { useState, type ReactNode } from 'react';
import { RANGES } from '../../../domain/hubState';
import { clusters } from '../../../domain/selectors';
import { useClient, useStore, useWholeState } from '../../../state/context';
import { CommitInput, TagList } from '../../../ui/inputs';
import { Toggle } from '../../../ui/toggle';
import { useSession } from '../../../app/session';
import { RegistryForm } from '../../registry/RegistryForm';
import { PageHead } from '../PageHead';

const Row = ({ title, desc, children }: { title: string; desc: string; children: ReactNode }) => (
  <div className="row"><div><b>{title}</b><span className="d">{desc}</span></div>{children}</div>
);

export function Settings() {
  return (
    <>
      <PageHead title="Settings" />
      <div className="settings-cols">
        <section><TvSettings /></section>
        <section><RegistrySettings /></section>
      </div>
    </>
  );
}

function RegistrySettings() {
  const [saved, setSaved] = useState(false);
  return (
    <>
      <h2 className="section-h">Registry</h2>
      <div className="card">
        <p className="help" style={{ marginTop: 0 }}>
          The container registry the agent images are pulled from. Add source uses it for the image, the version list and the login the cluster needs. Sources already added keep the image they were installed with.
        </p>
        <RegistryForm submitLabel="Save" onSaved={() => setSaved(true)} />
        {saved ? <p className="help" role="status" style={{ marginBottom: 0 }}>Saved.</p> : null}
      </div>
    </>
  );
}

function TvSettings() {
  const store = useStore();
  const client = useClient();
  const { auth, refresh } = useSession();
  const state = useWholeState();
  const s = state.settings;
  const set = (patch: Partial<typeof s>) => store.updateSettings((cur) => ({ ...cur, ...patch }));
  return (
    <>
      <h2 className="section-h">TV display</h2>
      <div className="card" style={{ maxWidth: 760 }}>
        <Row title="View without login" desc="Lets anyone who can reach this hub see the wallboard (#/tv) and its read-only data. Turn off if the hub is reachable by people you do not trust; then the TV needs to be logged in.">
          <Toggle checked={auth.publicView} label="View without login" onChange={(on) => void client.auth.setPublicView(on).then(refresh).catch(() => {})} />
        </Row>
        <Row title="Auto-rotate clusters" desc="Cycles All → each cluster. Pauses automatically during a critical incident."><Toggle checked={s.rotate} label="Auto-rotate" onChange={(on) => set({ rotate: on })} /></Row>
        <Row title="Rotation interval" desc="Seconds per view."><CommitInput type="number" min={5} value={s.rotateSec} style={{ width: 90 }} onCommit={(v) => set({ rotateSec: Number(v) })} /></Row>
        <Row title="Incident sidebar" desc="Host list and incidents on the right."><Toggle checked={s.sidebar} label="Sidebar" onChange={(on) => set({ sidebar: on })} /></Row>
        <Row title="Alarm sound" desc="Beeps on critical alerts. Browsers need one click on the TV page to allow audio."><Toggle checked={s.sound} label="Alarm sound" onChange={(on) => set({ sound: on })} /></Row>
        <Row title="System pods" desc="Show the cluster's own pods (kube-system, calico-system…) in the lists, on the map and on the TV. Hidden by default so your apps stand out; a system pod that is failing is always shown."><Toggle checked={s.showSystem} label="System pods" onChange={(on) => set({ showSystem: on })} /></Row>
        <Row title="System namespaces" desc="Which Kubernetes namespaces count as system. Enter or comma adds one; × removes it.">
          <TagList value={s.systemNamespaces} onChange={(v) => set({ systemNamespaces: v })} placeholder="namespace…" />
        </Row>
        <Row title="Default time range" desc="Which range Dashboard and the TV start on. Auto picks the smallest one that already shows all recorded history.">
          <div className="tabs">
            <button className={`pill${s.defaultRange === '' ? ' on' : ''}`} onClick={() => set({ defaultRange: '' })}>Auto</button>
            {RANGES.map((r) => <button key={r.id} className={`pill${s.defaultRange === r.id ? ' on' : ''}`} onClick={() => set({ defaultRange: r.id })}>{r.id}</button>)}
          </div>
        </Row>
        <Row title="Keep incident history" desc="Days a resolved incident stays in Alerts and can be opened as a report. Incidents still open are never forgotten."><CommitInput type="number" min={1} max={3650} value={s.incidentDays} style={{ width: 90 }} onCommit={(v) => set({ incidentDays: Math.max(1, Math.min(3650, Math.round(Number(v)) || 30)) })} /></Row>
        <div className="row">
          <div><b>Clusters on the TV</b><span className="d">Untick to hide a cluster from the wallboard.</span></div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, alignItems: 'flex-end' }}>
            {clusters(state).length
              ? clusters(state).map((c) => <label key={c.id} style={{ display: 'flex', gap: 10, alignItems: 'center' }}>{c.name}<Toggle checked={s.clusters[c.id] !== false} label={`Show ${c.name}`} onChange={(on) => set({ clusters: { ...s.clusters, [c.id]: on } })} /></label>)
              : <span className="muted">No clusters yet</span>}
          </div>
        </div>
      </div>
      <p className="help" style={{ marginTop: 12 }}>Saved on the hub: every open TV updates immediately, on any device.</p>
    </>
  );
}
