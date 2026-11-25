// Settings → Single sign-on: who may sign in through an identity provider (OpenID Connect: Microsoft Entra ID, Google, Keycloak, Okta, ...).
// The provider says who a person is; what they may do is still decided here, by the policies of their groups.
import { useCallback, useEffect, useState } from 'react';
import type { ProviderInput } from '../../../generated/ProviderInput';
import type { ProviderTest } from '../../../generated/ProviderTest';
import type { ProviderView } from '../../../generated/ProviderView';
import { useClient } from '../../../state/context';
import { Dialog } from '../../../ui/dialog';
import { Icon } from '../../../ui/icons';
import { ListBar, usePaged } from '../../../ui/paging';
import { StatusChip } from '../../../ui/status';
import { errorText, useAsyncSubmit } from '../../../ui/hooks';
import { useAccess } from './Access';

const ENTRA_ISSUER = 'https://login.microsoftonline.com/TENANT-ID/v2.0';

const blank = (): ProviderInput => ({
  name: '', issuer: '', clientId: '', secret: '', scopes: 'openid profile email', groupsClaim: 'groups', allowedDomains: '',
  enabled: true, autoCreate: true, linkExisting: true, trustEmail: false, enforce: false, defaultGroup: null, groupMap: [],
});

const inputOf = (p: ProviderView): ProviderInput => ({
  name: p.name, issuer: p.issuer, clientId: p.clientId, secret: '', scopes: p.scopes, groupsClaim: p.groupsClaim, allowedDomains: p.allowedDomains,
  enabled: p.enabled, autoCreate: p.autoCreate, linkExisting: p.linkExisting, trustEmail: p.trustEmail, enforce: p.enforce, defaultGroup: p.defaultGroup, groupMap: p.groupMap,
});

export function SsoTab() {
  const client = useClient();
  const { groups } = useAccess();
  const [providers, setProviders] = useState<ProviderView[]>([]);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState<ProviderView | 'new' | null>(null);
  const [removing, setRemoving] = useState<ProviderView | null>(null);
  const reload = useCallback(async () => {
    try { setProviders(await client.sso.providers()); setError(''); } catch (e) { setError(errorText(e)); }
  }, [client]);
  useEffect(() => { void reload(); }, [reload]);
  const matches = useCallback((p: ProviderView, text: string) => `${p.name} ${p.issuer}`.toLowerCase().includes(text), []);
  const paged = usePaged(providers, matches);
  return (
    <>
      {error ? <div className="err" role="alert">{error}</div> : null}
      <p className="help">
        People sign in at their company's identity provider and come back here. The provider says who they are; <b>what they may do</b> is decided by the groups they are in
        (map the provider's groups to groups here, or leave them in <b>everyone</b>). Break-glass accounts always keep their password, so a provider that is down or set up wrong
        never locks everybody out.
      </p>
      <div className="card">
        <div className="card-head"><h3>Identity providers</h3><button className="btn primary xs" onClick={() => setEditing('new')}>+ Add provider</button></div>
        <ListBar paged={paged} label="Search providers" placeholder="Search by name or address…" />
        {paged.total ? (
          <table className="tbl">
            <thead><tr>{['Provider', 'Issuer', ''].map((t) => <th key={t}>{t}</th>)}</tr></thead>
            <tbody>
              {paged.shown.map((p) => (
                <tr key={p.id}>
                  <td>
                    <span className="who"><Icon name="shield" size={16} /><b>{p.name}</b></span>{' '}
                    <StatusChip status={p.enabled ? 'ok' : 'unknown'}>{p.enabled ? 'On' : 'Off'}</StatusChip>{' '}
                    {p.enforce ? <StatusChip status="warn">Passwords: break-glass only</StatusChip> : null}
                  </td>
                  <td><code>{p.issuer}</code></td>
                  <td className="r">
                    <button className="btn xs" onClick={() => setEditing(p)}>Edit</button>{' '}
                    <button className="btn xs danger" onClick={() => setRemoving(p)}>Delete</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : <div className="empty"><b>{providers.length ? 'No provider matches' : 'No identity provider yet'}</b>{providers.length ? 'Try another search.' : 'Add Microsoft Entra ID, Google, Keycloak or any OpenID Connect provider.'}</div>}
      </div>
      {editing ? <EditProvider key={editing === 'new' ? 'new' : editing.id} provider={editing === 'new' ? null : editing} groups={groups.map((g) => ({ id: g.id, name: g.name }))} onClose={() => setEditing(null)} onDone={reload} /> : null}
      {removing ? <DeleteProvider key={removing.id} provider={removing} onClose={() => setRemoving(null)} onDone={reload} /> : null}
    </>
  );
}

function EditProvider({ provider, groups, onClose, onDone }: { provider: ProviderView | null; groups: { id: number; name: string }[]; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const [f, setF] = useState<ProviderInput>(provider ? inputOf(provider) : blank());
  const [test, setTest] = useState<ProviderTest | null>(null);
  const set = (patch: Partial<ProviderInput>) => setF((cur) => ({ ...cur, ...patch }));
  const { busy, error, submit } = useAsyncSubmit(async () => {
    const input = { ...f, groupMap: f.groupMap.filter((m) => m.claim.trim()) };
    if (provider) await client.sso.update(provider.id, input); else await client.sso.create(input);
    await onDone();
    onClose();
  });
  const check = () => { setTest(null); client.sso.test(f.issuer).then(setTest).catch((e) => setTest({ ok: false, message: errorText(e) })); };
  const check1 = (label: string, key: 'enabled' | 'autoCreate' | 'linkExisting' | 'trustEmail' | 'enforce', hint: string) => (
    <label className="check"><input type="checkbox" checked={f[key]} onChange={(e) => set({ [key]: e.target.checked })} /><span><b>{label}</b><span className="muted">{hint}</span></span></label>
  );
  return (
    <Dialog open onClose={onClose} wide>
      <h2>{provider ? provider.name : 'Add identity provider'}</h2>
      <form className="form" onSubmit={submit}>
        {provider ? (
          <p className="help" style={{ margin: 0 }}>Register this address at the provider as the <b>redirect URI</b> (Entra: Authentication → Web): <code>{provider.redirectUri}</code></p>
        ) : (
          <p className="help" style={{ margin: 0 }}>
            Create the app at the provider first, with the redirect URI this hub shows once saved. <button type="button" className="btn xs" onClick={() => set({ name: f.name || 'Microsoft Entra ID', issuer: ENTRA_ISSUER, trustEmail: true })}>Fill in for Microsoft Entra ID</button>
          </p>
        )}
        <label className="f">Name (what the login button says)<input value={f.name} onChange={(e) => set({ name: e.target.value })} autoComplete="off" /></label>
        <label className="f">Issuer address
          <input value={f.issuer} onChange={(e) => { set({ issuer: e.target.value }); setTest(null); }} placeholder="https://login.microsoftonline.com/<tenant id>/v2.0" autoComplete="off" />
        </label>
        <div><button type="button" className="btn xs" disabled={!f.issuer.trim()} onClick={check}>Test connection</button>{' '}
          {test ? <span role="status" className={test.ok ? 'ok' : 'err'}>{test.message}</span> : null}</div>
        <label className="f">Client (application) ID<input value={f.clientId} onChange={(e) => set({ clientId: e.target.value })} autoComplete="off" /></label>
        <label className="f">Client secret{provider?.hasSecret ? ' (leave empty to keep the saved one)' : ''}
          <input type="password" value={f.secret} onChange={(e) => set({ secret: e.target.value })} autoComplete="new-password" />
        </label>
        <label className="f">Allowed email domains (optional, separated by commas)<input value={f.allowedDomains} onChange={(e) => set({ allowedDomains: e.target.value })} placeholder="example.com" autoComplete="off" /></label>
        <div className="form" style={{ gap: 10 }}>
          {check1('Enabled', 'enabled', 'People can sign in with it.')}
          {check1('Create accounts on first sign-in', 'autoCreate', 'Off: only people who already have an account here (or are linked by email) can come in.')}
          {check1('Link local accounts with the same email', 'linkExisting', 'The account keeps its groups and history; its password goes, and it signs in through the provider from then on.')}
          {check1('Trust the provider’s email', 'trustEmail', 'For a company tenant. Off: only an email the provider says it verified can link an account.')}
          {check1('Passwords for break-glass accounts only', 'enforce', 'While this provider is on, nobody else may log in with a password.')}
        </div>
        <label className="f">Groups claim<input value={f.groupsClaim} onChange={(e) => set({ groupsClaim: e.target.value })} autoComplete="off" /></label>
        <label className="f">Scopes<input value={f.scopes} onChange={(e) => set({ scopes: e.target.value })} autoComplete="off" /></label>
        <label className="f">New accounts go into the group
          <select value={f.defaultGroup ?? ''} onChange={(e) => set({ defaultGroup: e.target.value ? Number(e.target.value) : null })}>
            <option value="">none (only everyone)</option>
            {groups.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
          </select>
        </label>
        <fieldset className="f">
          <legend>Groups from the provider → groups here (at every sign-in, a person is put in or taken out of these)</legend>
          {f.groupMap.map((m, i) => (
            <div key={i} style={{ display: 'flex', gap: 8 }}>
              <input aria-label={`Provider group ${i + 1}`} placeholder="value of the claim (Entra: the group's object ID)" value={m.claim} onChange={(e) => set({ groupMap: f.groupMap.map((x, j) => (j === i ? { ...x, claim: e.target.value } : x)) })} />
              <select aria-label={`Group here ${i + 1}`} value={m.group} onChange={(e) => set({ groupMap: f.groupMap.map((x, j) => (j === i ? { ...x, group: Number(e.target.value) } : x)) })}>
                {groups.map((g) => <option key={g.id} value={g.id}>{g.name}</option>)}
              </select>
              <button type="button" className="btn xs" aria-label={`Remove mapping ${i + 1}`} onClick={() => set({ groupMap: f.groupMap.filter((_, j) => j !== i) })}>×</button>
            </div>
          ))}
          <div><button type="button" className="btn xs" disabled={!groups.length} onClick={() => set({ groupMap: [...f.groupMap, { claim: '', group: f.defaultGroup ?? groups[0]!.id }] })}>+ Add mapping</button></div>
        </fieldset>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>{provider ? 'Save' : 'Add provider'}</button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteProvider({ provider, onClose, onDone }: { provider: ProviderView; onClose: () => void; onDone: () => Promise<void> }) {
  const client = useClient();
  const { busy, error, submit } = useAsyncSubmit(async () => {
    await client.sso.remove(provider.id);
    await onDone();
    onClose();
  });
  return (
    <Dialog open onClose={onClose}>
      <h2>Delete {provider.name}?</h2>
      <p className="help" style={{ marginTop: 0 }}>
        Nobody can sign in with it any more. The accounts that came through it stay, with their groups, but have no way in until a provider is back (or you give them another way).
      </p>
      <form onSubmit={submit}>
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn danger solid" disabled={busy}>Delete provider</button>
        </div>
      </form>
    </Dialog>
  );
}
