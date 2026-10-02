// The login to the container registry the agent images come from (Docker Hub, Harbor, Gitea, GHCR...). Used twice: as the second step of
// the first-run setup and as a tab in Settings. Nothing is saved until "Save"; "Test connection" tries what is typed.
import { useEffect, useState, type ReactNode } from 'react';
import type { RegistryInput } from '../../generated/RegistryInput';
import type { RegistryTest } from '../../generated/RegistryTest';
import type { RegistryView } from '../../generated/RegistryView';
import { useClient } from '../../state/context';
import { errorText, useAsyncSubmit } from '../../ui/hooks';

const Field = ({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) => (
  <label className="f">{label}{children}{hint ? <span className="muted" style={{ fontSize: 11.5 }}>{hint}</span> : null}</label>
);

// What a hub with nothing saved starts with: the official images, public on GHCR. Anyone with their own registry picks another preset.
const DEFAULT_REGISTRY = { url: 'ghcr.io', project: 'goformusic', auth: 'none' } as const;

const PRESETS: readonly { name: string; url: string; project?: string; hint: string }[] = [
  { name: 'Docker Hub', url: 'docker.io', hint: 'Project = your Docker Hub user or organization.' },
  { name: 'GitHub (GHCR)', url: 'ghcr.io', project: DEFAULT_REGISTRY.project, hint: 'The official images are public: no login. For your own images: User = your GitHub user, password = a token with read:packages.' },
  { name: 'Gitea', url: '', hint: 'Address = your Gitea host. User + a token with read:package.' },
  { name: 'Harbor', url: '', hint: 'Address = your Harbor host. Use a robot account with pull rights only.' },
];

export function RegistryForm({ submitLabel, onSaved, extra }: { submitLabel: string; onSaved: (saved: RegistryView) => void; extra?: ReactNode }) {
  const client = useClient();
  const [form, setForm] = useState<RegistryInput | null>(null);
  const [hasSecret, setHasSecret] = useState(false);
  const [result, setResult] = useState<RegistryTest | null>(null);
  const [testing, setTesting] = useState(false);
  const [hint, setHint] = useState('');

  useEffect(() => {
    let live = true;
    void client.registry.get().then((v) => {
      if (!live) return;
      const fresh = !v.url.trim(); // nothing saved yet: start from the official images instead of an empty form
      setForm({ ...(fresh ? DEFAULT_REGISTRY : { url: v.url, project: v.project, auth: v.auth }), username: v.username, linuxImage: v.linuxImage, windowsImage: v.windowsImage });
      setHasSecret(v.hasSecret);
    }).catch(() => {});
    return () => { live = false; };
  }, [client]);

  const { busy, error, submit } = useAsyncSubmit(
    async () => { onSaved(await client.registry.save(form!)); },
    () => (form && !form.url.trim() ? 'Type the registry address, or skip this step.' : null),
  );
  if (!form) return null;
  const set = (patch: Partial<RegistryInput>) => { setForm({ ...form, ...patch }); setResult(null); };
  const basic = form.auth === 'basic';

  const test = async () => {
    setTesting(true);
    try { setResult(await client.registry.test(form)); } catch (e) { setResult({ ok: false, message: errorText(e), versions: [] }); } finally { setTesting(false); }
  };

  return (
    <form onSubmit={submit}>
      <div className="form">
        <div className="tabs" role="group" aria-label="Registry presets">
          {PRESETS.map((p) => <button key={p.name} type="button" className={`pill${p.url && form.url === p.url ? ' on' : ''}`} onClick={() => { setHint(p.hint); if (p.url) set({ url: p.url, ...(p.project ? { project: p.project } : {}) }); }}>{p.name}</button>)}
        </div>
        {hint ? <div className="help" style={{ margin: 0 }}>{hint}</div> : null}
        <Field label="Registry address" hint="Host and port, e.g. git.example.com or harbor.example.com:8443. Write http:// in front for a plain, unencrypted registry.">
          <input type="text" placeholder="git.example.com" value={form.url} onChange={(e) => set({ url: e.target.value })} />
        </Field>
        <Field label="Project / user" hint="Where the images live: the part between the address and the image name.">
          <input type="text" placeholder="acm" value={form.project} onChange={(e) => set({ project: e.target.value })} />
        </Field>
        <Field label="Login">
          <select value={basic ? 'basic' : 'none'} onChange={(e) => set({ auth: e.target.value })}>
            <option value="none">None (public registry)</option>
            <option value="basic">User and password / token</option>
          </select>
        </Field>
        {basic ? (
          <>
            <Field label="User"><input type="text" autoComplete="off" value={form.username} onChange={(e) => set({ username: e.target.value })} /></Field>
            <Field label="Password or token" hint="Pull (read) rights are enough: use a robot account or a read-only token. Stored encrypted on the hub; the clusters use it to download the agent images.">
              <input type="password" autoComplete="new-password" placeholder={hasSecret ? '•••••••• saved (type to change)' : ''} value={form.secret ?? ''} onChange={(e) => set({ secret: e.target.value })} />
            </Field>
          </>
        ) : null}
        <details>
          <summary className="muted" style={{ cursor: 'pointer' }}>Image names</summary>
          <div className="form" style={{ marginTop: 8 }}>
            <Field label="Linux agent"><input type="text" value={form.linuxImage} onChange={(e) => set({ linuxImage: e.target.value })} /></Field>
            <Field label="Windows agent"><input type="text" value={form.windowsImage} onChange={(e) => set({ windowsImage: e.target.value })} /></Field>
          </div>
        </details>
        {result ? (
          <div className={result.ok ? 'help' : 'err'} role="status" style={{ margin: 0 }}>
            {result.message}
            {result.ok && result.versions.length ? <> · versions: {result.versions.slice(0, 6).join(', ')}{result.versions.length > 6 ? '…' : ''}</> : null}
          </div>
        ) : null}
        <div className="err" role="alert">{error}</div>
        <div className="dlg-actions" style={{ justifyContent: 'flex-start' }}>
          <button type="button" className="btn" disabled={testing || !form.url.trim()} onClick={() => void test()}>{testing ? 'Testing…' : 'Test connection'}</button>
          <button type="submit" className="btn primary" disabled={busy}>{submitLabel}</button>
          {extra}
        </div>
      </div>
    </form>
  );
}
