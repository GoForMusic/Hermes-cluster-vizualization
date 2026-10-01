## What and why

<!-- What changes, and the problem it solves or the need it meets. Link an issue if there is one (Closes #123). -->

## How it was tested

<!-- `cargo test --workspace`? `npm test` in hermes-server/web? Checked by hand against a real cluster (which one)? -->

## Checklist

- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` are clean
- [ ] `cargo test --workspace` passes
- [ ] If `hermes-server/web/` changed: `npm run typecheck`, `npm test`, `npm run build` pass, and any generated types (`hermes-server/web/src/generated/`) are committed
- [ ] If `proto/agent.proto` changed: only fields/cases were added, nothing renumbered or removed (an older agent must still work with a newer hub — see the rules at the top of the file)
