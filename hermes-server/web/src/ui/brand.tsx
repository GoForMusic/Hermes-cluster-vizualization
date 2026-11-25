// The HERMES wordmark/logo.
import { withBase } from '../hub/base';
export function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <span className={`brand-lockup${compact ? ' compact' : ''}`}>
      <span className="brand-banner" role="img" aria-label="HERMES, messenger of the cluster">
        <img src={withBase('/assets/hermes-banner-transparent.png')} alt="" />
      </span>
      <span className="logo brand-mark" aria-hidden="true" />
    </span>
  );
}
