# Contributing to HERMES

## Before you start

- For a bug or a small, obviously-right fix, just open a pull request.
- For anything bigger (a new feature, a behavior change, a new dependency), open an issue first — check the [existing issues](https://github.com/GoForMusic/Hermes-cluster-vizualization/issues) too, it might already be planned, with notes on why it isn't done yet. Saves you from building something that gets redesigned in review.
- Windows support (`agent-windows/`) is compiled and linted in CI but has never run on a real Windows node — if you can test it there, that's especially welcome.

## Set up

Open the folder in VS Code and "Reopen in Container" (`.devcontainer/`): Rust, Node and `protoc` are already in it, nothing else to install. See **Develop** in [README.md](README.md) for the commands (`cargo test --workspace`, running the hub, hot-reloading HERMES), and [TEST.md](TEST.md) for a full step-by-step install on a real Kubernetes or Swarm cluster.

## Making a change

- **Tests are never in the same file as the code.** Rust unit tests live in `<crate>/tests/unit/` (one file per module, pulled in with `#[path]` so they can still see the module's private functions), integration tests in `<crate>/tests/*.rs`, web tests in `hermes-server/web/src/test/`.
- **Comments explain *why*, not *what*.** Code should read clearly enough that a comment restating it is unnecessary; write one only for a non-obvious constraint, invariant, or workaround.
- **The wire protocol is backward compatible, always.** A change to `proto/agent.proto` may only add fields or oneof cases — never renumber or remove one — so a newer hub keeps accepting an older agent. The rules are at the top of that file, and `pkg/proto/tests/compat.rs` pins the contract.
- **Generated TypeScript types are committed**, not built on the fly: if you change a Rust type with `#[ts(export)]`, run the hub's tests (`cargo test -p hermes-hub`) to regenerate `hermes-server/web/src/generated/`, and commit the result.
- Keep changes scoped to what the issue or bug needs — this codebase prefers three similar lines over a premature abstraction, and no feature flags or back-compat shims where you can just change the code.

## Before opening the PR

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
# if hermes-server/web/ changed:
cd hermes-server/web && npm run typecheck && npm test && npm run build
```

CI (`ci.yml`) runs the same checks, scoped to what your PR touches, and reports results as a comment on the PR. `ci-ok` is the one required check.

## Pull requests

Fill in the PR template — what changed and why, how you tested it. One pull request per logical change is easier to review than one that bundles several.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). Be direct about the technical disagreement, respectful about the person.
