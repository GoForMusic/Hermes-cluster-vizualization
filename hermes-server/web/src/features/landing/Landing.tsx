import { useHubState } from '../../state/context';

export function Landing() {
  const version = useHubState((s) => s.info.version);
  return (
    <div className="landing">
      <div>
        <h1 className="landing-logo"><img src="/assets/hermes-banner-transparent.png" alt="HERMES: messenger of the cluster" /></h1>
        <p>A live map of your Kubernetes clusters, Docker Swarm clusters and Docker machines: what runs where, what is using what, and what just broke.</p>
      </div>
      <div className="landing-cards">
        <a className="landing-card" href="#/tv"><b>TV display</b><span>Read-only wallboard for a TV: every cluster at a glance, with incidents as they happen.</span></a>
        <a className="landing-card" href="#/admin"><b>Admin panel</b><span>Sources, alert rules, topology, uptime history and TV settings. Needs a login.</span></a>
      </div>
      {version ? <div className="landing-ver">HERMES v{version}</div> : null}
    </div>
  );
}
