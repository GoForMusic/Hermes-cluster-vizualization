// The wallboard: read-only, no login needed, meant for a TV on the LAN. Laid out like a CS HUD: the map in the middle, the kill feed top-right,
// big amber readouts at the bottom, scoreboard and console on the side. Composition/layout only — the 5 widgets each live in their own file.
import { useEffect, useMemo, useState } from 'react';
import { dtg } from '../../domain/format';
import type { HubState } from '../../domain/hubState';
import { PROVIDERS } from '../../domain/providers';
import { activeAlerts, visibleClusters } from '../../domain/selectors';
import { ICON } from '../../domain/status';
import { shallowEqual, useHubState, useStore } from '../../state/context';
import { beep } from '../../ui/audio';
import { useNow } from '../../ui/hooks';
import { Icon } from '../../ui/icons';
import { Brand } from '../../ui/brand';
import { StatusTags } from '../shared/StatusTags';
import { Legend } from '../topology/Legend';
import { TopologyMap } from '../topology/TopologyMap';
import { useKillFeed } from './useKillFeed';
import { useRotation } from './useRotation';
import { Banner } from './Banner';
import { HudBar } from './HudBar';
import { Scoreboard } from './Scoreboard';
import { Traffic } from './Traffic';
import { Console } from './Console';

const INSET = { top: 84, bottom: 84 };

const settingsOf = (s: HubState) => s.settings;
const clusterKey = (s: HubState) => visibleClusters(s).map((c) => `${c.id}:${c.status}:${c.name}:${c.provider}`);

export function TvScreen() {
  const store = useStore();
  const settings = useHubState(settingsOf);
  const clusterKeys = useHubState(clusterKey, shallowEqual);
  const clusters = useMemo(() => clusterKeys.map((k) => { const [id = '', status = 'ok', name = '', provider = 'kubernetes'] = k.split(':'); return { id, status, name, provider }; }), [clusterKeys]);
  const alerts = useHubState((s) => activeAlerts(s), shallowEqual);
  const crit = alerts.filter((a) => a.sev === 'crit');
  const warn = alerts.filter((a) => a.sev === 'warn');
  const clusterIds = useMemo(() => clusters.map((c) => c.id), [clusters]);
  const visible = useMemo(() => new Set(clusterIds), [clusterIds]);

  const { focusId, choose } = useRotation({ enabled: settings.rotate, seconds: settings.rotateSec, holding: crit.length > 0, clusterIds });
  const feed = useKillFeed(settings.sound);
  const [legendOpen, setLegendOpen] = useState(false);
  const now = useNow(1000);
  const d = new Date(now);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key.toLowerCase() === 'l') setLegendOpen((o) => !o); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const focusName = focusId ? clusters.find((c) => c.id === focusId)?.name : 'All clusters';
  const rotate = !settings.rotate ? 'Rotate off' : crit.length ? 'Hold · incident' : `Rotate ${settings.rotateSec}s`;

  return (
    <div className={`tv${crit.length ? ' has-crit' : ''}${settings.sidebar ? '' : ' no-side'}`}>
      <header className="tv-top">
        <div className="seg tv-title"><Brand /></div>
        <div className="seg tv-pills">
          {[{ id: null, name: 'All clusters', status: null, provider: null }, ...clusters].map((c) => (
            <button key={c.id ?? 'all'} className={`pill${focusId === c.id ? ' on' : ''}${c.status ? ` st-${c.status}` : ''}`} title={c.provider ? PROVIDERS[c.provider as keyof typeof PROVIDERS].label : 'Show everything'} onClick={() => choose(c.id)}>
              {c.status ? <span className="dot" /> : null}
              {c.provider ? <Icon name={PROVIDERS[c.provider as keyof typeof PROVIDERS].icon} size={14} color={PROVIDERS[c.provider as keyof typeof PROVIDERS].color} /> : null}
              {c.name}
            </button>
          ))}
        </div>
        <div className="strip">
          {crit.length || warn.length ? null : <div className="nominal">✓ All systems nominal</div>}
          <div className={`cnt st-crit${crit.length ? ' on' : ''}`}><small>Crit</small><b>{crit.length}</b></div>
          <div className={`cnt st-warn${warn.length ? ' on' : ''}`}><small>Warn</small><b>{warn.length}</b></div>
        </div>
        <div className="seg tv-clock"><b>{d.toLocaleTimeString('en-GB', { hour12: false })}</b><span>DTG {dtg(d)}</span></div>
      </header>

      <div className="tv-main">
        <div className="tv-graph-wrap">
          <div className="tv-graph"><TopologyMap visible={visible} focusId={focusId} inset={INSET} /></div>
          {clusters.length ? null : <div className="empty-map"><div><b>No data sources yet</b>Add a Kubernetes cluster in <a href="#/admin/sources">Admin → Sources</a>.</div></div>}
          <Banner crit={crit} warn={warn} />
          <div className="killfeed">
            {feed.map((f) => <div key={f.id} className={`kf ${f.cls}`}><span>{ICON[f.cls]}</span>{f.text}{f.sub ? <small>{f.sub}</small> : null}</div>)}
          </div>
          <HudBar />
          <Legend open={legendOpen} onOpenChange={setLegendOpen} />
        </div>
        <aside className="tv-side">
          <Scoreboard />
          <Traffic />
          <section className="panel grow"><div className="panel-h">Incidents</div><div className="panel-b" style={{ padding: 0 }}><Console /></div></section>
        </aside>
      </div>

      <footer className="statusbar">
        <StatusTags />
        <span className="hud">View: {focusName}</span>
        <span>{rotate}</span>
        <span className="spacer" />
        <button onClick={() => { store.updateSettings((s) => ({ ...s, sound: !s.sound })); if (!settings.sound) beep(); }}>{settings.sound ? 'Sound on' : 'Sound off'}</button>
        <button onClick={() => setLegendOpen((o) => !o)}>L Legend</button>
        <a href="#/tv/status">Status view ↗</a>
        <a href="#/admin">Admin ↗</a>
      </footer>
    </div>
  );
}
