# Security

## Reporting a vulnerability

Please **don't** open a public issue for a security vulnerability. Use GitHub's private reporting instead:

[Report a vulnerability](https://github.com/GoForMusic/Hermes-cluster-vizualization/security/advisories/new)

Include what you found, how to reproduce it, and the affected version (hub/agent). We'll acknowledge the report, work out a fix, and credit you in the advisory unless you'd rather stay anonymous.

## Scope

HERMES is designed with a specific trust model, worth knowing before reporting:

- **The hub never holds cluster write credentials.** Agents connect outbound only and report read-only data; the hub cannot run commands in your cluster. An exception: if you opt in to "Allow upgrades from the dashboard" for a source, the hub can tell that source's own agent to change its own image — nothing else.
- **Credentials at rest** (registry passwords, agent tokens) are encrypted in SQLite with `HUB_SECRET_KEY` (AES-256-GCM). A vulnerability that bypasses this encryption, or that lets one source read another source's secrets, is in scope.
- **The agent's RBAC is intentionally minimal** (read-only, plus two named objects when self-upgrade is on). A way to escalate beyond what the generated manifest grants is in scope.
- Supported versions: the latest release of each component (`hermes-server`, `hermes-agent-linux`, `hermes-agent-windows`). We don't backport fixes to older tags.
