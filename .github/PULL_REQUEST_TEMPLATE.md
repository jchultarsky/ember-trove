## Summary

<!-- What does this change and why? Link the issue it addresses, if any. -->

## Checklist

- [ ] Branched from `develop`, targets `develop` (see [CONTRIBUTING.md](../CONTRIBUTING.md))
- [ ] Conventional commit messages (`feat|fix|refactor|test|docs|chore|ci(scope): …`)
- [ ] Tests pass locally (`cargo test --workspace --exclude ui`)
- [ ] Clippy clean, both targets (`cargo clippy --workspace --exclude ui -- -D warnings`
      and `cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings`)
- [ ] Formatted (`cargo fmt --all --check`)
- [ ] New code lands with tests; bug fixes include a regression test
- [ ] No `.unwrap()` / `.expect()` / `panic!` in non-test code
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] New env vars / endpoints / commands documented in `README.md`

## Security

<!-- This app stores personal data. Does this change touch authN/authZ, query
scoping, SQL, markdown rendering, logging, or dependencies? If yes, say how the
security posture is preserved. If no, state "No security-relevant surface." -->
