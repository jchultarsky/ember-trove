# Contributing to Ember Trove

Thank you for your interest in contributing! Please read this guide before opening
a pull request.

---

## Code of Conduct

Be respectful and constructive. Harassment of any kind is not tolerated.

---

## Development Setup

### Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install sqlx-cli --features postgres
```

### Local Stack

```bash
docker compose -f deploy/docker-compose.yml up -d postgres minio keycloak
export DATABASE_URL=postgres://ember_trove:ember_trove_dev@localhost/ember_trove
sqlx migrate run --source migrations/
```

See [README.md](README.md) for the full step-by-step local development guide, including Keycloak setup.

---

## Guardrails (see [`.claude/POLICY.md`](.claude/POLICY.md) for the full policy)

- **Plan first** for anything beyond a one-line fix; **reuse** existing code (grep before writing).
- **Zero panics** — no `.unwrap()`/`.expect()`/`panic!` in non-test code. Use `?` everywhere.
- **`thiserror`** for library error types; **`anyhow`** for application glue.
- **TDD** — write a failing test first; new code and bug fixes land with tests.
- **Research crates** (docs.rs, MSRV, license, advisories) before adding one; prefer `std`
  and existing `[workspace.dependencies]`. Track the latest **stable** Rust.

**Install the git hooks once**, then they run the gates for you:

```bash
make hooks-install   # pre-commit: cargo fmt --all --check + clippy; pre-push: cargo test
```

Formatting is enforced (default `rustfmt`, edition 2024). Run the full suite anytime with
`make verify` (or `./scripts/verify.sh`):

```bash
cargo fmt --all --check && cargo clippy -- -D warnings && cargo check && cargo test
```

---

## Git Flow

This project follows [Git Flow](https://nvie.com/posts/a-successful-git-branching-model/).
`v1.0.0` is the first production tag on `main`.

| Branch type | Pattern              | Branched from | Merges into       | Notes                                      |
|-------------|----------------------|---------------|-------------------|--------------------------------------------|
| Feature     | `feature/jc/<name>`  | `develop`     | `develop`         | `--no-ff`; delete branch + worktree after  |
| Release     | `release/<semver>`   | `develop`     | `main` + `develop`| Tag `v<semver>` on `main` after merge      |
| Hotfix      | `hotfix/<name>`      | `main`        | `main` + `develop`| Tag patch bump on `main` after merge       |

### Rules

- **Never commit directly to `main` or `develop`** — all changes must go through a branch.
- **Feature branches**: branch from `develop`, merge back to `develop` with `--no-ff`, then delete.
- **Release branches**: bump version in `Cargo.toml`, update `CHANGELOG.md`, merge into `main` (tag) and back into `develop`, then delete.
- **Hotfix branches**: branch from the tagged commit on `main`, fix, merge into `main` (tag patch bump) and back into `develop`, then delete.
- Never force-push `main` or `develop`.

### Example: shipping a feature

```bash
# 1. Branch from develop
git checkout develop
git pull origin develop
git checkout -b feature/jc/my-feature

# 2. Work, commit, push
git add <files>
git commit -m "feat(api): add my feature"

# 3. Merge back to develop (no fast-forward)
git checkout develop
git merge --no-ff feature/jc/my-feature -m "chore: merge feature/jc/my-feature into develop"

# 4. Clean up
git branch -d feature/jc/my-feature
git push origin develop
```

### Example: cutting a release

```bash
# 1. Branch from develop
git checkout develop
git checkout -b release/1.1.0

# 2. Bump version, update CHANGELOG.md, commit
git commit -am "chore(release): prepare v1.1.0"

# 3. Merge into main and tag
git checkout main
git merge --no-ff release/1.1.0 -m "chore: merge release/1.1.0 into main"
git tag -a v1.1.0 -m "v1.1.0"

# 4. Merge back into develop
git checkout develop
git merge --no-ff release/1.1.0 -m "chore: merge release/1.1.0 back into develop"

# 5. Clean up and push
git branch -d release/1.1.0
git push origin main develop --tags
```

### Example: applying a hotfix

```bash
# 1. Branch from main at the tagged release
git checkout main
git checkout -b hotfix/fix-auth-cookie

# 2. Fix, commit
git commit -am "fix(auth): correct cookie expiry calculation"

# 3. Merge into main and tag
git checkout main
git merge --no-ff hotfix/fix-auth-cookie -m "chore: merge hotfix/fix-auth-cookie into main"
git tag -a v1.0.1 -m "v1.0.1"

# 4. Merge back into develop
git checkout develop
git merge --no-ff hotfix/fix-auth-cookie -m "chore: merge hotfix/fix-auth-cookie into develop"

# 5. Clean up and push
git branch -d hotfix/fix-auth-cookie
git push origin main develop --tags
```

---

## Pull Request Checklist

- [ ] Tests pass (`cargo test`)
- [ ] Clippy clean (`cargo clippy -- -D warnings`)
- [ ] Formatted (`cargo fmt`)
- [ ] CHANGELOG.md updated under `[Unreleased]`
- [ ] New environment variables documented in README.md

---

## Commit Message Style

```
<type>(<scope>): <short description>

[optional body]

Co-Authored-By: <name> <email>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `ci`.

Example:

```
feat(api): add node CRUD endpoints with permission checks
```
