# Ember Trove — Qwen Knowledge Base

> Rust/Leptos full-stack knowledge graph app. Three-crate workspace: `api/` (Axum REST), `common/` (shared DTOs), `ui/` (Leptos WASM CSR). PostgreSQL 16, S3 attachments, Cognito OIDC auth. Latest release: **v1.75.12**.

---

## Toolchain & Build

- **Rust**: Pinned to `1.92` (rust-toolchain.toml) with `wasm32-unknown-unknown` target + `clippy` component.
- **Edition**: `2024` across all three crates.
- **Profile**: Custom `[profile.dist]` inherits `release` with `lto = "thin"`.
- **UI build**: Must compile with `--target wasm32-unknown-unknown`. Excluded from default `cargo test`.
- **Key deps**: Axum 0.8, sqlx 0.8, Leptos 0.8, Tailwind v4, AWS SDK (requires >= 1.91.1).

---

## CI/CD — GitHub Actions

### ci.yml — runs on push/PR to `main` and `develop`

| Job | Details |
|-----|---------|
| **check** | Matrix (ubuntu + macos): `cargo check`, `clippy`, `test` for api+common (excludes ui) |
| **check-wasm** | Ubuntu only: `cargo check` + `clippy` for ui with `wasm32-unknown-unknown` |
| **audit** | Ubuntu only: `cargo audit` for security vulnerabilities |
| **migrations** | Ubuntu + PostgreSQL 16 service: `sqlx migrate run` to validate migrations |
| **docker-build** | Ubuntu only: builds Dockerfile.api + Dockerfile.ui (no push) to catch Dockerfile errors |

### release.yml — runs on push of version tags (`v*.*.*`)

| Job | Details |
|-----|---------|
| **release** | Extracts release notes from CHANGELOG.md, creates GitHub Release via `gh release create` |
| **build** | Needs: release. Patches api/Cargo.toml version, logs into GHCR, builds & pushes Docker images (api + ui) to `ghcr.io/jchultarsky/ember-trove-{api,ui}` using Docker Buildx with GHA cache |
| **deploy** | Needs: build, requires `DEPLOY_ENABLED=true`. SSH to Lightsail EC2, pulls pre-built images, fetches latest compose config from git main, restarts via docker-compose, verifies `localhost:3003/api/health`, cleans old images. Concurrency: `production-ec2-deploy` |

### Key gotchas
- **Force-pushing tags**: If a tag already has a release, `gh release create` will fail with "tag_name already exists". Fix: `gh release delete vX.Y.Z --yes --cleanup-tag`, then re-push tag.
- **Actions versions**: checkout@v6, docker/build-push-action@v7, docker/login-action@v4, docker/setup-buildx-action@v4 (all Node.js 24 compatible).
- **Env vars**: `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24`, `REGISTRY=ghcr.io`, `API_IMAGE`, `UI_IMAGE`.

---

## Git Workflow

- **Branches**: `main` (production), `develop` (integration). Features: `feature/jc/<name>` from `develop`. Releases: `release/<semver>` from `develop`, merged to `main` with tag. Hotfixes: `hotfix/<name>` from `main`.
- **Syncing develop**: `git checkout develop && git merge --ff-only main && git push origin develop`
- **Release flow**: Bump `api/Cargo.toml` version → add CHANGELOG entry → commit → tag `vX.Y.Z` → push commit + tag.

### Git Worktrees — Best Practice

Worktrees let you check out multiple branches in parallel without stashing or switching directories. They share the same `.git` object store, so there's no refetch.

**When to use worktrees:**
- **Hotfix while working on a feature** — `git worktree add ../hotfix-tmp main`
- **Code review** — `git worktree add ../review-tmp feature/pr-branch`
- **Agent sessions** — AI tools can work in isolated worktrees without disrupting main work
- **Parallel experiments** — two feature branches side-by-side for comparison

**Creating a worktree:**
```bash
# Standard: checkout a new branch
git worktree add ../ember-trove-feat feature/new-thing

# Detached HEAD for quick inspection
git worktree add --detach ../tmp-check

# With an existing branch
git worktree add ../ember-trove-fix hotfix/urgent
```

**Cleaning up (mandatory):**
```bash
rm -rf ../path-to-worktree     # Remove the working directory first
git worktree prune             # Clean up git's internal refs
git branch -d feature-branch   # Delete the branch if merged
git branch -D feature-branch   # Force delete if unmerged
```

**⚠️ Gotchas:**
- **Branches in use by worktrees cannot be deleted** — `git branch -d` will error
- **Orphaned worktrees** (deleted without `prune`) block branch deletion and clutter `git branch -a`
- **Agent auto-cleanup** — AI tools may create worktrees and leave them behind. Always check `git worktree list` before releasing branches.
- **`.claude/` cleanup** — orphaned Claude sessions leave stale data in `.claude/projects/` and `.claude/worktrees/`. Clean periodically:
  ```bash
  rm -rf .claude/projects/* .claude/worktrees/*
  ```
- **IDE confusion** — some editors lock onto the original repo's `.git`. Open the worktree root, not the parent.

---

## Architecture Overview

### Crate Structure

| Crate | Purpose | Key pattern |
|-------|---------|-------------|
| `api/` | Axum 0.8 REST backend | Three-tier router (public → rate-limited → protected) |
| `common/` | Shared DTOs, newtype IDs, error types, validation | 22 modules, all shared between api+ui |
| `ui/` | Leptos 0.8 CSR/WASM frontend | Signal-based state management, global keyboard shortcuts |

### API Route Registration (`api/src/routes/mod.rs`)

Three-tier router with layer stack (outermost → innermost):
```
PropagateRequestIdLayer → SetRequestIdLayer → TimeoutLayer(30s) → CorsLayer → GovernorLayer → require_auth → handlers
```

- **Public routes**: `/health`, `/share/*`, `/auth/login`, `/auth/callback`, `/auth/refresh`, `/auth/logout` — no auth required
- **Rate-limited routes**: Public + all protected behind `GovernorLayer` (10 req/s per IP, burst 100, POST/PUT/DELETE/PATCH only)
- **Protected routes**: Everything else behind `require_auth` middleware

Each domain module exposes `pub fn router() -> Router<AppState>`, merged via `.nest("/nodes", nodes::router())`. Health endpoint is on a separate sub-router to exclude from rate limiting.

### Error Handling (`api/src/error.rs`)

Two-layer model:
- **`common::EmberTroveError`** — domain errors (`NotFound`, `AlreadyExists`, `Unauthorized`, `Forbidden`, `Validation`, `Internal`). Used by repo layer.
- **`api::error::ApiError`** — HTTP errors (adds `Database`, `Storage`, `Conflict`). `impl From<EmberTroveError>` bridges layers.
- `impl IntoResponse for ApiError` → `(StatusCode, Json({ "error": message }))`.
- `sqlx::Error` auto-converts via `#[from]`. Internal errors log details server-side, return generic messages to clients.

### Authentication (`api/src/auth/`)

**OIDC with PKCE** against AWS Cognito:
1. `GET /auth/login` → generates PKCE code_verifier/challenge, stores in `PkceStore` (HashMap), returns Cognito auth URL
2. Browser → Cognito hosted UI → user authenticates
3. `GET /auth/callback` → exchanges code + PKCE verifier for tokens, sets three HttpOnly cookies:
   - `ember_trove_session` — ID token (path `/`)
   - `ember_trove_refresh` — refresh token (path `/api/auth/refresh`)
   - `ember_trove_access` — Cognito access token (path `/api/auth/change-password`)
4. **`require_auth` middleware** (`middleware.rs`): Extracts JWT from session cookie first, then `Authorization: Bearer` header. Validates via JWKS (cached 1 hour). Maps `cognito:groups` → `AuthClaims.roles`.
5. **Permissions** (`permissions.rs`): Role hierarchy `Viewer < Editor < Owner`. `require_role()` checks admin bypass first. Helpers: `require_viewer`, `require_editor`, `require_owner`.

---

## UI Architecture

### App Root (`ui/src/app.rs`)

**Context signals** (provided at App root, consumed via `use_context`):
| Signal | Purpose |
|--------|---------|
| `Theme` | Persisted in localStorage, toggles `dark` class on `<html>` |
| `current_view: RwSignal<View>` | View routing |
| `refresh: RwSignal<u32>` | Bump to re-fetch nodes list |
| `tag_filter` / `node_type_filter` | Filter state for NodeList/SearchView |
| `search_query: RwSignal<String>` | Shared between SearchBar and SearchView |
| `task_refresh` | Task list refresh newtype wrapper |
| `show_capture` | Quick-capture modal visibility |
| `toast_state` | Notification system |
| `template_prefill` | Template pre-fill for NodeEditor |
| `app_version` | Fetched from `/api/health` at startup |
| `current_node_pinned` | Pin state for global `p` shortcut |

### View enum (`ui/src/app.rs`)

```rust
pub enum View {
    NodeList, NodeDetail(NodeId), NodeCreate, NodeEdit(NodeId),
    TagManager, Graph, Search, Admin, ProjectDashboard,
    MyDay, Calendar, Notes, Backup, Templates, BulkPermissions,
}
```

### Global Keyboard Shortcuts (document-level, suppressed in editable elements)

| Key | Action |
|-----|--------|
| `n` | Quick-capture modal |
| `g` | Graph view |
| `/` | Search (prevents browser find) |
| `d` | Duplicate current node |
| `p` | Toggle pin |
| `Escape` | Back to node list / close shortcuts modal |
| `?` | Toggle shortcuts help modal |

### Auth State (`ui/src/auth.rs`)

`AuthStatus` enum: `Loading | Authenticated(UserInfo) | Unauthenticated`. `provide_auth_state()` creates signal, provides context, kicks off `GET /api/auth/me`.

### API Client (`ui/src/api.rs`)

Uses `gloo_net::http::Request` for all HTTP calls. `parse_json<T>()` helper handles 401 by attempting `refresh_session()` then full page reload. All functions are `async fn` returning `Result<T, UiError>`.

---

## Common Types (`common/src/`)

**22 modules** exported. Key ones:

| Module | Key Types | Gotcha |
|--------|-----------|--------|
| `id.rs` | `uuid_newtype!` macro → 14 newtypes: `NodeId`, `EdgeId`, `TagId`, `AttachmentId`, `PermissionId`, `TaskId`, `NoteId`, `FavoriteId`, `ShareTokenId`, `ActivityId`, `NodeVersionId`, `TemplateId`, `SearchPresetId`, `NodeLinkId` | All `[serde(transparent)]`, with `Display`, `FromStr`, `Default` |
| `node.rs` | `NodeType` (Article/Project/Area/Resource/Reference), `NodeStatus` (Draft/Published/Archived), `Node`, CRUD requests | `NodeType`/`NodeStatus` serialize as `snake_case` (lowercase!) |
| `permission.rs` | `PermissionRole` (Owner/Editor/Viewer), `Permission`, Grant/Update requests | |
| `activity.rs` | `ActivityAction` (11 variants), `ActivityEntry` | |
| `edge.rs` | `Edge`, `EdgeWithTitles`, `CreateEdgeRequest` | |
| `tag.rs` | `Tag`, `CreateTagRequest`, `UpdateTagRequest` | |
| `task.rs` | `Task`, `MyDayTask` (with `#[serde(flatten)]` on `task`), `CreateTaskRequest`, `UpdateTaskRequest`, `TaskCounts`, `ProjectDashboardEntry` | |
| `graph.rs` | `NodePosition`, `SavePositionRequest`, `SavePositionsRequest` | |
| `error.rs` | `EmberTroveError` enum | |
| `auth.rs` | `AuthClaims`, `UserInfo`, `ChangePasswordRequest` | |

### Serde gotchas
- `NodeType`, `NodeStatus`, `PermissionRole` serialize as `snake_case` → use lowercase in payloads.
- `NodeListParams.subject_id` has `#[serde(skip)]` → set server-side from JWT, never from client.
- `MyDayTask` uses `#[serde(flatten)]` on `task` field → flattens task fields into parent object.

---

## Database Layer (`api/src/repo/`)

**18 repo files**, each following the same pattern:

1. **Trait definition** — `pub trait XRepo: Send + Sync` with `#[async_trait]` methods returning `Result<T, EmberTroveError>`
2. **Concrete struct** — `pub struct PgXRepo { pool: PgPool }` with `fn new(pool) -> Self`
3. **Intermediate row types** — `#[derive(sqlx::FromRow)]` structs with `sqlx(default)` for optional fields
4. **String-cast enum parsing** — PostgreSQL custom enums stored as text, parsed manually in Rust
5. **Batch tag fetching** — `fetch_tags_for_nodes()` helper does single query for multiple nodes, returns `HashMap<Uuid, Vec<Tag>>`

**`PgPool` usage**: Shared `PgPool` cloned into each repo. Max 10 connections. Migrations run at startup via `sqlx::migrate!("../migrations").run(&pool)`.

**23 migrations** (`migrations/`), numbered 001–023.

### Adding a batch API endpoint (pattern from v1.75.12)

1. **common**: Add request struct with `#[derive(Serialize, Deserialize)]`
2. **repo trait**: Add method signature returning `Result<(), EmberTroveError>`
3. **PgXRepo impl**: Implement with transaction if multi-row (`pool.begin()` → operations → `tx.commit()`)
4. **route**: Add `Router::new().route("/path", put(handler))` handler
5. **ui/api.rs**: Add `pub async fn save_xxx(...)` using `gloo_net::http::Request`
6. **tests**: Update `StubXRepo` with the new trait method

### Testing patterns (`api/src/tests.rs`)

- **Integration tests** via `tower::ServiceExt::oneshot`: Full production router with `oidc = None` (auth middleware returns 500 for all protected routes)
- **Stub repositories**: Each repo trait implemented with `unimplemented!()` — never reached because auth short-circuits
- `assert_route_registered()` helper: sends request, asserts status != 404 (500 means route exists)
- `Config { ..Config::default() }` pattern — adding new optional fields never breaks tests

---

## AppState (`api/src/state.rs`)

Single struct with: all repo traits as `Arc<dyn Trait>`, `PgPool`, `Arc<dyn ObjectStore>`, optional `OidcClient`/`CognitoAdminClient`/`SesNotifier`, `cookie_key: Key`, `auth: AuthConfig`, `config: Config`, `pkce_store: PkceStore`. `impl FromRef<AppState> for Key` enables `PrivateCookieJar` extraction.

---

## Graph View (`ui/src/components/graph_view.rs`)

### Auto-arrange algorithm
Pure **BFS hierarchical layering** — NO force simulation.
- Root nodes (in-degree 0) → top row
- BFS layers fan out below
- Hubs centered within each layer
- Disconnected components tiled in a grid
- Auto-fit centers graph in viewport with 0.5x minimum zoom
- Spacing: `NODE_W=80px`, `LAYER_SPACING=100px`, `COMPONENT_SPACING=200px`
- Function: `smart_layout()` — returns positions, pan, zoom for auto-fit
- Canvas: 3000×2000 virtual SVG with auto-grow (up to 4× for 200+ nodes)
- Zoom range: 0.05× to 16×, manual zoom input (editable number field, Enter to set)

### Persistence
- Positions load from `GET /graph/positions` on mount
- Positions save to `PUT /graph/positions/{node_id}` on drag (mouse-up)
- **Auto-arrange** saves ALL positions via `PUT /graph/positions` (batch) in a single transaction
- Toolbar: unified glassmorphic container (`bg-white/85 dark:bg-stone-900/90 backdrop-blur-md rounded-xl shadow-lg`) with h-8 buttons separated by `border-r` dividers

### Node shapes by type
- Circle = Article, Diamond = Project, Rounded-rect = Area, Hexagon = Resource, Triangle = Reference
- Each type has a distinct fill colour

---

## UI Icon Button Pattern (unified across all sections)

| Button | Icon | Base classes | Hover |
|--------|------|-------------|-------|
| **Save** | `check` | `p-1.5 rounded-lg text-stone-400` | Green (`hover:bg-green-50 dark:hover:bg-green-900/30 hover:text-green-600`) |
| **Cancel** | `close` | `p-1.5 rounded-lg text-stone-400` | Stone (`hover:bg-stone-100 dark:hover:bg-stone-800`) |
| **Add/New toggle** | `add` ↔ `close` | Same as cancel | Same as cancel |
| **Disabled save** | `hourglass_empty` | Same + `disabled:opacity-50` | None |

All use **Material Symbols Outlined** icons.

---

## Configuration (`api/src/config.rs`)

`Config::from_env()` reads from environment. `Config::default()` returns zero-value config for tests. Key vars: `DATABASE_URL`, `COOKIE_KEY` (128 hex chars), `FRONTEND_URL`, `API_EXTERNAL_URL`, `OIDC_*`, `S3_*`, `COGNITO_*`, `SES_FROM_EMAIL`. Optional fields default to `None` — services are gracefully disabled.

---

## Deployment

### Local dev (`deploy/docker-compose.yml`)
- PostgreSQL 16, MinIO (S3-compatible), api, ui services
- MinIO-init creates bucket on first boot
- UI served on port 8003, API on 3003
- `--env-file deploy/.env.local` for secrets

### Production (`deploy/docker-compose.prod.yml`)
- Pulls pre-built images from GHCR (`ghcr.io/jchultarsky/ember-trove-{api,ui}`)
- nginx reverse proxy with Let's Encrypt certificates
- AWS S3 (not MinIO), Cognito OIDC

### Docker images — multi-stage builds
- **API**: `rust:latest` → `debian:trixie-slim`. Dependency caching via stub sources.
- **UI**: `rust:latest` (with `trunk` via `cargo-binstall`) → `nginx:alpine`

---

## Key Gotchas & Patterns

1. **`NodeListParams.subject_id`** has `#[serde(skip)]` — set server-side from JWT, never from client
2. **Admin bypass** — `claims.roles.contains("admin")` skips per-node permission checks and list filters
3. **Fire-and-forget patterns** — activity logging and version snapshots use `tokio::spawn` with warning-on-failure
4. **String-cast PostgreSQL enums** — custom types stored as text, parsed manually in Rust (not sqlx `PgHasArrayType`)
5. **Batch tag fetching** — single query with `WHERE nt.node_id = ANY($1)` instead of N+1
6. **PKCE over cookies for OAuth state** — avoids iOS Safari ITP cookie restrictions
7. **Refresh token rotation** — Cognito rotates refresh tokens; cookie updated on each refresh
8. **Health endpoint exempt from rate limiting** — separate sub-router
9. **`NullObjectStore`** returns errors when S3 not configured; routes that need it fail gracefully
10. **`Config::default()`** designed so `..Config::default()` in tests never breaks when new fields added
11. **Rust 2024 edition lints** — `collapsible_if` is now enabled by default with `-D warnings`. Nested `if let` blocks that can be collapsed with `&&` will fail CI.
12. **Tag release conflicts** — force-pushing a tag after a release already exists causes `gh release create` to fail with "tag_name already exists". Delete old release + tag first.

---

## Housekeeping Checklist

When doing maintenance passes:
- [ ] **Dead code** — search for unused functions/constants (especially after algorithm swaps like force→BFS layout)
- [ ] **Version drift** — `api/Cargo.toml` version should match latest git tag
- [ ] **Edition consistency** — all crates should use the same edition
- [ ] **Changelog gaps** — compare `git tag` versions against CHANGELOG.md entries
- [ ] **Module doc comments** — update if implementation changed (e.g., "Fruchterman-Reingold" → "BFS hierarchical")
- [ ] **Run clippy** — `cargo clippy -p api -- -D warnings` + `cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings`
- [ ] **Run tests** — `cargo test --workspace --exclude ui`
- [ ] **Clean branches** — `git branch --merged main`, prune worktrees, delete dangling remotes
