// Step 2 of the first-run setup (step 1 is the admin account): where the agent images come from. It can be skipped and done later in Settings.
import { Brand } from '../../ui/brand';
import { RegistryForm } from './RegistryForm';

export function SetupRegistry({ onDone }: { onDone: () => void }) {
  return (
    <div className="auth-wrap">
      <div className="card auth-card">
        <div className="auth-brand"><Brand compact /></div>
        <p className="muted" style={{ margin: 0 }}>Step 2 of 2</p>
        <h1>Where do the agent images come from?</h1>
        <p className="help">
          Log in to the container registry that holds the Hermes agent images (Docker Hub, Harbor, Gitea, GHCR…). When you add a source, the hub then writes the install command with the right image and
          the login the cluster needs to download it. You can change it any time in Settings → Registry.
        </p>
        <RegistryForm submitLabel="Save and continue" onSaved={onDone} extra={<button type="button" className="btn" onClick={onDone}>Skip for now</button>} />
      </div>
    </div>
  );
}
