## Summary

<!-- What does this change and why? Link the issue it addresses, if any. -->

Proposal: #<!-- issue number, if this implements a proposal; else delete this line -->

## Checklist

- [ ] Branched from `develop`, targets `develop` (see [CONTRIBUTING.md](../CONTRIBUTING.md))
- [ ] Conventional commit messages (`feat|fix|refactor|test|docs|chore|ci(scope): …`)
- [ ] Tests pass locally (`cargo test --workspace --exclude ui`)
- [ ] Clippy clean, both targets (`cargo clippy --workspace --exclude ui --all-targets -- -D warnings`
      and `cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings`)
- [ ] Formatted (`cargo fmt --all --check`)
- [ ] New code lands with tests; bug fixes include a regression test
- [ ] No `.unwrap()` / `.expect()` / `panic!` in non-test code
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] Issue linked with `Fixes #n` (the release closes it) or `Refs #n` (it stays open: partial fix or awaiting a manual re-test)
- [ ] New env vars / endpoints / commands documented in `README.md`

## Security

<!-- This app stores personal data. Does this change touch authN/authZ, query
scoping, SQL, markdown rendering, logging, or dependencies? If yes, say how the
security posture is preserved. If no, state "No security-relevant surface." -->
