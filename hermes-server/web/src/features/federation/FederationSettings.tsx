// Settings → Federation: the datacenters this hub controls (member hubs in network zones the agents cannot reach this hub from), or, for a hub
// that is about to be managed by another one, its pairing code.
import { useState } from 'react';
import type { DatacenterView } from '../../generated/DatacenterView';
import type { MemberCode } from '../../hub/HubClient';
import { useClient, useStore } from '../../state/context';
import { Dialog } from '../../ui/dialog';
import { errorText, useAsyncSubmit } from '../../ui/hooks';
import { StatusChip } from '../../ui/status';
import { useFederation } from './useFederation';

const STATE_CHIP = {
  connected: <StatusChip status="ok">Connected</StatusChip>,
  connecting: <StatusChip status="unknown">Connecting</StatusChip>,
  unreachable: <StatusChip status="crit">Unreachable</StatusChip>,
} as const;

export function FederationSettings() {
  const client = useClient();
  const store = useStore();
  const { view, reload } = useFederation(true, 4000);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<DatacenterView | null>(null);
  const [editing, setEditing] = useState<DatacenterView | null>(null);
  const [becoming, setBecoming] = useState(false);
  const [code, setCode] = useState<MemberCode | null>(null);
  const [note, setNote] = useState('');
  const [error, setError] = useState('');

  const changed = async () => { await reload(); await store.refreshSources(); };
  const run = async (work: () => Promise<void>) => { setError(''); try { await work(); } catch (e) { setError(errorText(e)); } };

  return (
    <>
      <h2 className="section-h">Federation</h2>
      <div className="card">
        {view.member ? (
          <>
            <p className="help" style={{ marginTop: 0 }}>
              This hub is waiting for the hub that will manage it. There, open <b>Settings → Federation → Add datacenter</b> and give it the address <code>{view.member.agentAddress}</code> and this code.
              When it connects, this hub locks: no login here, everything is done from the other hub.
            </p>
            {code ? <div className="pair-box"><span className="muted">Pairing code</span><code className="pair-code" data-testid="pair-code">{code.code}</code></div> : null}
            <div className="dlg-actions" style={{ justifyContent: 'flex-start' }}>
              <button className="btn primary" onClick={() => void run(async () => { setCode(await client.federation.newCode()); })}>{code ? 'New code' : 'Show a code'}</button>
              <button className="btn" onClick={() => void run(async () => { await client.federation.disableMember(); setCode(null); await reload(); })}>Stop waiting</button>
            </div>
          </>
        ) : (
          <>
            <p className="help" style={{ marginTop: 0 }}>
              A datacenter is another hub that sits next to machines this hub cannot reach (a firewall lets this hub connect to it, not the other way round). This hub connects to it, shows what it sees
              and manages it: sources, agent versions and its registry are all done from here. Its agents report only to it.
            </p>
            {view.datacenters.length ? (
              <div className="dc-list">
                {view.datacenters.map((d) => (
                  <div key={d.id} className="dc-row">
                    <div>
                      <b>{d.name}</b> {STATE_CHIP[d.state as keyof typeof STATE_CHIP] ?? STATE_CHIP.connecting}
                      <div className="src-meta">{d.address}{d.version ? ` · hub v${d.version}` : ''} · {d.sources} source{d.sources === 1 ? '' : 's'}</div>
                      {d.state !== 'connected' ? <div className="src-meta">{d.info}</div> : null}
                    </div>
                    <div className="src-actions" style={{ marginTop: 0, paddingTop: 0 }}>
                      <button className="btn xs" onClick={() => setEditing(d)}>Change address</button>
                      <button className="btn xs danger" onClick={() => setRemoving(d)}>Remove</button>
                    </div>
                  </div>
                ))}
              </div>
            ) : <p className="muted">No datacenters yet.</p>}
            <div className="dlg-actions" style={{ justifyContent: 'flex-start' }}>
              <button className="btn primary" onClick={() => setAdding(true)}>+ Add datacenter</button>
              {view.datacenters.length ? null : <button className="btn" onClick={() => setBecoming(true)}>Let another hub manage this one…</button>}
            </div>
          </>
        )}
        {note ? <p className="help" role="status" style={{ marginBottom: 0 }}>{note}</p> : null}
        <div className="err" role="alert">{error}</div>
      </div>
      <AddDatacenterDialog open={adding} onClose={() => setAdding(false)} onAdded={async () => { setAdding(false); setNote(''); await changed(); }} />
      <EditDatacenterDialog dc={editing} onClose={() => setEditing(null)} onSaved={async () => { setEditing(null); await changed(); }} />
      <RemoveDatacenterDialog
        dc={removing}
        onClose={() => setRemoving(null)}
        onConfirm={(d) => void run(async () => {
          setRemoving(null);
          const { released } = await client.federation.removeDatacenter(d.id);
          setNote(released ? `${d.name} is a hub on its own again.` : `${d.name} could not be told: it stays locked until it is restarted once with HUB_FEDERATION=off.`);
          await changed();
        })}
      />
      <BecomeMemberDialog open={becoming} onClose={() => setBecoming(false)} onCode={async (c) => { setBecoming(false); setCode(c); await reload(); }} />
    </>
  );
}

function AddDatacenterDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded: () => Promise<void> }) {
  const client = useClient();
  const [name, setName] = useState('');
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  const { busy, error, submit } = useAsyncSubmit(
    async () => {
      await client.federation.addDatacenter({ name: name.trim(), address: address.trim(), code: code.trim() });
      setName(''); setAddress(''); setCode('');
      await onAdded();
    },
    () => (name.trim() && address.trim() && code.trim() ? null : 'Fill in the name, the address and the pairing code.'),
  );
  return (
    <Dialog open={open} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Add datacenter</h2>
        <p className="help" style={{ marginTop: 0 }}>
          On the child hub (the one to add), choose “Managed by another hub” on its first start (or Settings → Federation → “Let another hub manage this one”). It shows an address and a pairing code.
        </p>
        <div className="form">
          <label className="f">Name<input type="text" placeholder="dc2" value={name} onChange={(e) => setName(e.target.value)} /></label>
          <label className="f">Address of the child hub<input type="text" placeholder="https://hermes.dc2.example:8765" value={address} onChange={(e) => setAddress(e.target.value)} /><span className="muted" style={{ fontSize: 11.5 }}>The address this hub connects to: the one the child hub shows on its pairing screen.</span></label>
          <label className="f">Pairing code<input type="text" autoComplete="off" placeholder="XXXX-XXXX-XXXX" value={code} onChange={(e) => setCode(e.target.value)} /></label>
          <div className="err" role="alert">{error}</div>
        </div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy}>{busy ? 'Connecting…' : 'Connect'}</button>
        </div>
      </form>
    </Dialog>
  );
}

function EditDatacenterDialog({ dc, onClose, onSaved }: { dc: DatacenterView | null; onClose: () => void; onSaved: () => Promise<void> }) {
  const client = useClient();
  const [address, setAddress] = useState('');
  const [shown, setShown] = useState<string | null>(null);
  if (dc && shown !== dc.id) { setShown(dc.id); setAddress(dc.address); }
  const { busy, error, submit } = useAsyncSubmit(async () => {
    if (dc) await client.federation.updateDatacenter(dc.id, { address: address.trim() });
    await onSaved();
  });
  return (
    <Dialog open={dc != null} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Address of {dc?.name}</h2>
        <div className="form">
          <label className="f">Address of the child hub<input type="text" value={address} onChange={(e) => setAddress(e.target.value)} /><span className="muted" style={{ fontSize: 11.5 }}>The address this hub connects to.</span></label>
          <div className="help" style={{ margin: 0 }}>The pairing stays: only where this hub connects to changes.</div>
          <div className="err" role="alert">{error}</div>
        </div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !address.trim() || address.trim() === dc?.address}>Save</button>
        </div>
      </form>
    </Dialog>
  );
}

function RemoveDatacenterDialog({ dc, onClose, onConfirm }: { dc: DatacenterView | null; onClose: () => void; onConfirm: (dc: DatacenterView) => void }) {
  return (
    <Dialog open={dc != null} onClose={onClose}>
      <h2>Remove {dc?.name}?</h2>
      <p className="help" style={{ marginTop: 0 }}>
        This hub stops showing it and its sources, and tells it to go back to being a hub on its own (it asks for a first admin again). Its agents keep reporting to it. Nothing is deleted there.
        If it cannot be reached it stays locked until it is restarted once with <code>HUB_FEDERATION=off</code>.
      </p>
      <div className="dlg-actions">
        <button type="button" className="btn" onClick={onClose}>Cancel</button>
        <button type="button" className="btn danger solid" onClick={() => dc && onConfirm(dc)}>Remove datacenter</button>
      </div>
    </Dialog>
  );
}

function BecomeMemberDialog({ open, onClose, onCode }: { open: boolean; onClose: () => void; onCode: (c: MemberCode) => Promise<void> }) {
  const client = useClient();
  const [address, setAddress] = useState(location.origin);
  const { busy, error, submit } = useAsyncSubmit(async () => { await onCode(await client.federation.enableMember(address.trim())); });
  return (
    <Dialog open={open} onClose={onClose}>
      <form onSubmit={submit}>
        <h2>Let another hub manage this one</h2>
        <p className="help" style={{ marginTop: 0 }}>
          This hub shows a pairing code and waits for the other hub to connect with it. From then on it has <b>no login</b>: its sources, registry and settings are controlled from the other hub,
          and the accounts here stop working. Remove it there to get it back.
        </p>
        <div className="form">
          <label className="f">Address of this hub<input type="text" value={address} onChange={(e) => setAddress(e.target.value)} /><span className="muted" style={{ fontSize: 11.5 }}>How the other hub and the agents next to this one reach it. Not localhost.</span></label>
          <div className="err" role="alert">{error}</div>
        </div>
        <div className="dlg-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary" disabled={busy || !address.trim()}>Show the pairing code</button>
        </div>
      </form>
    </Dialog>
  );
}
