// Alert rules. Stored on the hub and evaluated there, so every browser and the TV agree.
import { useStore, useHubState } from '../../../state/context';
import { CommitInput } from '../../../ui/inputs';
import { StatusChip } from '../../../ui/status';
import { Toggle } from '../../../ui/toggle';
import { PageHead } from '../PageHead';

export function Rules() {
  const store = useStore();
  const rules = useHubState((s) => s.settings.rules);
  const change = (id: string, patch: { value?: number; enabled?: boolean }) => store.updateSettings((s) => ({ ...s, rules: s.rules.map((r) => (r.id === id ? { ...r, ...patch } : r)) }));
  return (
    <>
      <PageHead title="Alert rules" />
      <div className="card">
        <table className="tbl">
          <thead><tr>{['Rule', 'Condition', 'Severity', 'Enabled'].map((t) => <th key={t}>{t}</th>)}</tr></thead>
          <tbody>
            {rules.map((r) => (
              <tr key={r.id}>
                <td><b>{r.name}</b><div className="muted">Applies to: {r.target}</div></td>
                <td>
                  {r.cond}{' '}
                  {r.value != null ? <CommitInput type="number" min={1} value={r.value} onCommit={(v) => change(r.id, { value: Number(v) })} /> : null}{' '}
                  <span className="muted">{r.unit}</span>
                </td>
                <td><StatusChip status={r.sev}>{r.sev === 'crit' ? 'Critical' : 'Warning'}</StatusChip></td>
                <td><Toggle checked={r.enabled} onChange={(on) => change(r.id, { enabled: on })} label={`${r.name} enabled`} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="help" style={{ marginTop: 12 }}>Rules run in the hub. Changes apply immediately and are stored in its database.</p>
    </>
  );
}
