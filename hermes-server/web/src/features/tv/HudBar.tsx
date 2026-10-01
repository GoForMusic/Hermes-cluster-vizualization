// The big readouts along the bottom: hosts, workloads, traffic, storage.
import type { HubState } from '../../domain/hubState';
import { summary, type Summary } from '../../domain/selectors';
import { useHubState } from '../../state/context';
import { Icon } from '../../ui/icons';

function HudItem({ icon, label, value, unit, status }: { icon: string; label: string; value: string; unit?: string; status: string }) {
  return <div className={`hud-item st-${status}`}><Icon name={icon} size={30} /><small>{label}</small><b>{value}{unit ? <i>{unit}</i> : null}</b></div>;
}

const summaryOf = (s: HubState): Summary => summary(s);
const sameSummary = (a: Summary, b: Summary) => JSON.stringify(a) === JSON.stringify(b);

/** The fullest volume, not an average: with many volumes an average says nothing about the one that is about to fill up. */
function storageItem(s: Summary): { label: string; value: string; unit: string; status: string } {
  if (s.volumes === 0) return { label: 'Storage', value: '—', unit: '', status: 'ok' };
  if (s.fullest == null) return { label: `Volumes · ${s.volumes}`, value: s.volumeUsed.toFixed(1), unit: ' GiB', status: 'ok' }; // none has a limit
  const pct = s.fullest.pct;
  return { label: `Fullest · ${s.volumes} vol`, value: String(Math.round(pct)), unit: '%', status: pct >= 95 ? 'crit' : pct >= 85 ? 'warn' : 'ok' };
}

export function HudBar() {
  const s = useHubState(summaryOf, sameSummary);
  const traffic = !s.trafficKnown ? ['—', ''] : s.traffic >= 1000 ? [(s.traffic / 1000).toFixed(1), 'Gb/s'] : [String(Math.round(s.traffic)), 'Mb/s'];
  return (
    <div className="hud-bar">
      <div className="hud-group">
        <HudItem icon="server" label="Hosts" value={String(s.hostsUp)} unit={`/${s.hostsTotal}`} status={s.hostsUp < s.hostsTotal ? 'crit' : 'ok'} />
        <HudItem icon="cube" label="Workloads" value={String(s.wlRun)} unit={`/${s.wlTotal}`} status={s.wlRun < s.wlTotal ? 'warn' : 'ok'} />
      </div>
      <div className="hud-group">
        <HudItem icon="net" label="Traffic" value={traffic[0]!} unit={` ${traffic[1]}`} status="ok" />
        <HudItem icon="disk" {...storageItem(s)} />
      </div>
    </div>
  );
}
