// The tags of the status bar: is the feed live, is the data live, which hub version is this, and has the hub been updated under this page.
import { useHubState } from '../../state/context';
import type { HubState } from '../../domain/hubState';

const tags = (s: HubState) => `${s.link}|${s.info.demo}|${s.info.version}|${s.bootVersion}`;

export function StatusTags() {
  const [link, demo, version, bootVersion] = useHubState(tags).split('|') as [string, string, string, string];
  const lost = link === 'lost';
  const updated = version !== '' && bootVersion !== '' && version !== bootVersion; // the hub was updated while this page stayed open
  return (
    <>
      <span className="live-tag" style={lost ? { color: 'var(--crit)' } : undefined}>{lost ? 'Link lost — retrying' : 'Feed live'}</span>
      <span>{demo === 'true' ? 'Data: DEMO fixture' : 'Data: live'}</span>
      <span className="ver-tag">
        {updated
          ? <a href="#" className="ver-new" onClick={(e) => { e.preventDefault(); location.reload(); }}>Hub updated to v{version} — reload</a>
          : version ? `Hub v${version}` : ''}
      </span>
    </>
  );
}
