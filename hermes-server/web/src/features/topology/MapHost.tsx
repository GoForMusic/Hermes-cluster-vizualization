// A host's box on the map: the plain frame drawn under the links (`MapHost`), and its text/icons drawn over them afterwards
// (`HostLabel`) so a line passing behind a name never runs through it.
import { memo, type MouseEvent } from 'react';
import type { HostBox } from '../../domain/map/layout';
import type { Node } from '../../domain/model';
import { PROVIDERS, TF_COLOR } from '../../domain/providers';
import { IconG } from '../../ui/icons';

/** The box of a host. Its text is drawn later (`HostLabel`), over the links, so that a line passing behind a name does not run through it. */
export const MapHost = memo(function MapHost({ box, host, selected, onClick }: { box: HostBox; host: Node; selected: boolean; onClick: (e: MouseEvent) => void }) {
  const isNas = host.meta.role === 'storage';
  return (
    <g className={`ghost st-${host.status}${selected ? ' sel' : ''}`} data-id={host.id} onClick={onClick}>
      <title>{isNas ? 'NAS — a storage box on the network. Clusters mount its folders over NFS.' : `Host — a ${host.meta.role} machine (VM or PC) that runs workloads.`}</title>
      <rect className="host-box" x={box.x} y={box.y} width={box.w} height={box.h} />
      <rect className="host-strip" x={box.x + 0.6} y={box.y + 0.6} width={box.w - 1.2} height={27} />
    </g>
  );
});

export const HostLabel = memo(function HostLabel({ box, host, stat, selected, onClick }: { box: HostBox; host: Node; stat: string; selected: boolean; onClick: (e: MouseEvent) => void }) {
  const isNas = host.meta.role === 'storage';
  const iac = host.meta.iac;
  const tfW = iac?.drift ? 60 : 78;
  const tfX = box.x + box.w - 10 - tfW, tfY = box.y + 46;
  return (
    <g className={`ghost st-${host.status}${selected ? ' sel' : ''}`} data-id={host.id} onClick={onClick}>
      <IconG name={isNas ? 'nas' : 'server'} x={box.x + 8} y={box.y + 5} size={18} color={PROVIDERS[host.provider].color} />
      <text className="host-name" x={box.x + 32} y={box.y + 19}>{host.name}</text>
      {host.meta.osType ? <text className={`host-os ${host.meta.osType}`} x={box.x + box.w - 10} y={box.y + 19} textAnchor="end">{host.meta.osType.toUpperCase()}</text> : null}
      {iac ? (
        <g className="tf">
          <title>{iac.drift ? `Terraform drift: the real machine no longer matches the Terraform code. ${iac.note}` : `Managed by Terraform (${iac.ref}). The real machine matches the code.`}</title>
          <rect className={iac.drift ? 'tf-bg drift' : 'tf-bg'} x={tfX} y={tfY} width={tfW} height={17} />
          <IconG name="terraform" x={tfX + 5} y={tfY + 3} size={11} color={iac.drift ? 'var(--warn)' : TF_COLOR} />
          <text className={iac.drift ? 'tf-tx drift' : 'tf-tx'} x={tfX + 20} y={tfY + 12.5}>{iac.drift ? 'drift' : 'managed'}</text>
        </g>
      ) : null}
      <text className="host-sub" x={box.x + 10} y={box.y + 44}>{host.meta.ip} · {host.meta.role}</text>
      <text className="host-stat" x={box.x + 10} y={box.y + 59}>{stat}</text>
    </g>
  );
});
