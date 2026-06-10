# Ember Trove

[![Build Status](https://img.shields.io/github/actions/workflow/status/jchultarsky101/ember-trove/ci.yml?branch=main&style=flat-square)](https://github.com/jchultarsky101/ember-trove/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg?style=flat-square)](https://www.rust-lang.org/)
[![Leptos](https://img.shields.io/badge/leptos-0.8-purple.svg?style=flat-square)](https://leptos.dev/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-336791.svg?style=flat-square)](https://www.postgresql.org/)

> A self-hosted, graph-centric personal knowledge management system — your second brain, written in Rust.

---

## Overview

Ember Trove is a web-based personal knowledge management (PKM) application where **nodes** (articles, projects, areas, resources, references) are linked by **typed edges**, tagged with flexible metadata, and searchable via full-text + fuzzy search. Markdown is the primary authoring format. Files can be attached to any node and stored in S3-compatible object storage.

### Key Features

- **Graph-centric** — nodes and typed directional edges form a navigable knowledge graph with a visual graph view. Node shapes encode type; colours encode status. Click tag dots to filter the graph by tag.
- **Markdown-native** — split-pane editor with collapsible live preview (auto-hidden on mobile), rendered via `pulldown-cmark` + `ammonia`. Wiki-link `[[title]]` syntax auto-creates edges and provides autocomplete. **Drag-and-drop or paste images** directly into the editor to upload and embed them inline.
- **Full-text + fuzzy search** — PostgreSQL `tsvector` full-text search and `pg_trgm` trigram similarity, covering nodes, notes, and tasks. Length-normalised ranking with visual relevance indicator. Save and restore search presets.
- **Multi-tag filtering** — AND/OR tag filters across node list, search results, and graph view. Tags can be attached directly from the node list without opening the node.
- **S3 attachments** — bulk drag-and-drop upload; inline preview for images and PDFs. Stored in MinIO (local) or Lightsail Object Storage / AWS S3.
- **Tasks & My Day** — per-node task lists with create / toggle / edit / delete. Daily planning view aggregates today's tasks across all nodes.
- **Notes feed** — append-only timestamped notes per node, editable after creation, surfaced in a global newest-first feed.
- **Node versioning** — body snapshot on every save; version history timeline with one-click restore.
- **Activity log** — per-node audit trail of 10 action types (created, updated, tag changes, permission changes, attachments, etc.).
- **Node pinning** — pin important nodes; pinned nodes sort first in the list and are highlighted with an amber ring in the graph. `p` key toggles pin on the open node.
- **Node templates** — create reusable Markdown templates for each node type; "Use" pre-fills the editor body.
- **Quick capture** — `n` keyboard shortcut opens a lightweight modal for rapid node creation. Type, title, and optional body; Ctrl+Enter to save.
- **Keyboard shortcuts** — `n` new node · `g` graph · `/` search · `p` pin · `?` shortcut help · `Esc` back. All suppressed inside form fields.
- **Multi-user permissions** — nodes are **private by default**. Owners can invite others by email with Viewer / Editor / Owner roles. Bulk permission management available in the admin panel. Admin users have full access to all nodes regardless of ownership.
- **User management** — admin UI backed by Amazon Cognito. User invite emails via AWS SES.
- **Public share links** — generate opaque share tokens for read-only access to a node without login.
- **Export** — download any node as Markdown (with YAML front-matter) or JSON.
- **Light / dark mode** — class-based Tailwind v4 warm ember theme, persisted in `localStorage`.
- **Mobile-responsive** — hamburger top bar on mobile; sidebar slides in as an overlay.
- **Cognito hosted UI** — custom CSS and flame-icon logo matching the app's stone/amber palette.
- **Self-hosted** — fully Dockerised with both a local dev stack and a production AWS deployment guide. CD pipeline via GitHub Actions on `v*.*.*` tags.

---

## Tech Stack

| Layer       | Technology                              |
|-------------|-----------------------------------------|
| Backend     | Rust · Axum 0.8 · Tokio                 |
| Frontend    | Leptos 0.8 CSR/WASM · Tailwind CSS v4   |
| Database    | PostgreSQL 16 · sqlx 0.8               |
| File Store  | S3-compatible (MinIO / Lightsail Object Storage / AWS S3) |
| Auth        | Amazon Cognito (OIDC; hosted UI + custom CSS) |
| Markdown    | pulldown-cmark · ammonia               |
| OpenAPI     | utoipa + Swagger UI                     |
| Build       | Trunk (UI) · cargo workspace            |
| Deploy      | Docker multi-stage · Kubernetes         |

---

## Workspace Structure

```
ember-trove/
├── Cargo.toml            # workspace (api, ui, common)
├── common/               # shared DTOs, error types, ID newtypes
├── api/                  # Axum REST backend  — port 3003
├── ui/                   # Leptos/Trunk WASM  — port 8003
├── migrations/           # sqlx migrations (PostgreSQL schema, 017 migrations)
├── docs/                 # Deployment and operations guides
└── deploy/
    ├── Dockerfile.api
    ├── Dockerfile.ui
    ├── docker-compose.yml         # local development stack
    ├── docker-compose.prod.yml    # production AWS stack
    ├── nginx.conf                 # dev nginx config
    ├── nginx.prod.conf            # production nginx config (TLS)
    ├── .env.prod.template         # production env var template
    ├── cognito.css                # Cognito hosted UI custom stylesheet
    ├── logo.png                   # Cognito hosted UI logo
    └── backup.sh                  # PostgreSQL backup/restore to S3
```

---

## Production Deployment (AWS)

See **[docs/deploy-aws.md](docs/deploy-aws.md)** for a complete step-by-step guide to deploying on AWS Lightsail with Amazon Cognito and Lightsail Object Storage.

**Summary of the production stack:**

| Component | Service | Cost |
|-----------|---------|------|
| Compute | AWS Lightsail (4 GB / 2 vCPU) | ~$20/mo |
| Object Storage | Lightsail Object Storage 5 GB | ~$1/mo |
| Auth | Amazon Cognito (free ≤ 50 K MAU) | $0 |
| TLS | Let's Encrypt via Certbot | $0 |
| **Total** | | **~$21/mo** |

---

## Running Locally

There are two ways to run the stack on your machine:

- **[Option A — Full Docker stack](#option-a--full-docker-stack-recommended)** (recommended): everything in containers.
- **[Option B — Native API + UI](#option-b--native-api--ui-faster-iteration)**: the app from source, backing services in Docker (faster rebuilds).

Both need an OIDC identity provider for login — see **[Auth: bring your own Cognito](#auth-bring-your-own-cognito)** first.

### Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Docker Desktop | latest | [docs.docker.com/get-docker](https://docs.docker.com/get-docker/) |
| Rust | stable ≥ 1.91.1 | [rustup.rs](https://rustup.rs) — *native mode only* |
| wasm32 target | — | `rustup target add wasm32-unknown-unknown` — *native mode only* |
| Trunk | latest | `cargo install trunk` — *native mode only* |
| sqlx-cli | latest | `cargo install sqlx-cli --features postgres` — *only to author new migrations* |

> **Note:** `aws-sdk-s3` requires Rust ≥ 1.91.1. Run `rustup update stable` if your toolchain is older.
> Migrations are applied automatically at API startup (`sqlx::migrate!`), so you never run them by hand.

---

### Auth: bring your own Cognito

Ember Trove authenticates via **OIDC against Amazon Cognito**. It reads roles from the
Cognito `cognito:groups` claim and expects a Cognito **ID token**, so there is no
bundled local identity provider — to log in locally you point it at a Cognito user pool.

- **Maintainer:** use the existing pool. Put its app-client secret and (optionally) the
  admin IAM keys into `deploy/.env.local` (below). The pool/client IDs are already the
  defaults in `deploy/docker-compose.yml`.
- **Cloned the repo?** Create your own pool (one-time; Cognito is free ≤ 50 K MAU):
  1. **Cognito → Create user pool.** Note the **Pool ID** (`<region>_xxxxx`); the issuer
     is `https://cognito-idp.<region>.amazonaws.com/<pool-id>`.
  2. Add an **app client** of type *confidential* (with a client secret). Note the
     **client ID** and **secret**.
  3. Enable the **Authorization code grant** with scopes `openid email profile`, set up a
     **Hosted UI domain**, and add the allowed **callback URL**
     `http://localhost:8003/api/auth/callback` and sign-out URL `http://localhost:8003`.
  4. Create a group named **`admin`** and add your user to it — app roles come from
     `cognito:groups`.
  5. *(Optional)* For the `/api/admin/*` user-management endpoints, create an IAM user with
     Cognito admin permissions and use its keys as `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`.

  Then set `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, `COGNITO_USER_POOL_ID`, and
  `COGNITO_REGION` to your pool's values — in `deploy/docker-compose.yml` (the non-secret IDs)
  and `deploy/.env.local` (the secrets).

---

### Option A — Full Docker stack (recommended)

Runs every service as a container: PostgreSQL, MinIO (local S3, bucket auto-created), the
API, and the UI (nginx serving the WASM SPA and proxying `/api/`).

```bash
# 1. Create your local secrets file (gitignored).
cp deploy/.env.local.example deploy/.env.local

# 2. Fill it in: OIDC_CLIENT_SECRET, the two AWS_* keys (optional, for /admin),
#    and a COOKIE_KEY. Generate the cookie key — the API rejects a weak/all-identical one:
#      openssl rand -hex 64

# 3. Build and start the whole stack.
docker compose -f deploy/docker-compose.yml --env-file deploy/.env.local up --build
```

Open **http://localhost:8003**. (First build is slow — Rust + WASM compile.)

| Service    | URL                    | Credentials                       |
|------------|------------------------|-----------------------------------|
| UI (nginx) | http://localhost:8003  | — (log in via Cognito)            |
| API        | http://localhost:3003  | —                                 |
| MinIO API  | http://localhost:9000  | `ember_trove` / `ember_trove_dev` |
| MinIO UI   | http://localhost:9001  | `ember_trove` / `ember_trove_dev` |
| PostgreSQL | `localhost:5432`       | `ember_trove` / `ember_trove_dev` |

Stop with `docker compose -f deploy/docker-compose.yml down` (add `-v` to also wipe the
Postgres/MinIO volumes).

---

### Option B — Native API + UI (faster iteration)

Run only the backing services in Docker and the app from source — rebuilds are much faster
than re-baking images.

**1. Start Postgres + MinIO:**

```bash
docker compose -f deploy/docker-compose.yml up -d postgres minio
# Create the S3 bucket (the API does not auto-create it in native mode):
docker exec deploy-minio-1 mc alias set local http://localhost:9000 ember_trove ember_trove_dev
docker exec deploy-minio-1 mc mb --ignore-existing local/ember-trove
```

**2. Build and run the API** (migrations apply automatically on startup):

```bash
export DATABASE_URL=postgres://ember_trove:ember_trove_dev@localhost/ember_trove
export S3_ENDPOINT=http://localhost:9000
export S3_BUCKET=ember-trove
export S3_ACCESS_KEY=ember_trove
export S3_SECRET_KEY=ember_trove_dev
export S3_REGION=us-east-1
# Point these at YOUR Cognito pool (see "Auth: bring your own Cognito"):
export OIDC_ISSUER=https://cognito-idp.<region>.amazonaws.com/<pool-id>
export OIDC_CLIENT_ID=<your-app-client-id>
export OIDC_CLIENT_SECRET=<your-app-client-secret>
export COOKIE_KEY=$(openssl rand -hex 64)   # must be a real 128-hex-char key
export FRONTEND_URL=http://localhost:8003
export API_EXTERNAL_URL=http://localhost:8003
export COOKIE_SECURE=false                   # local dev is plain http
export RUST_LOG=info

cargo run -p api
```

Verify: `curl http://localhost:3003/api/health` → `{"status":"ok",...}`.
Swagger UI (loopback only): **http://localhost:3003/swagger-ui/**.

**3. Build and run the UI dev server** (`ui/Trunk.toml` proxies `/api` → `localhost:3003`):

```bash
trunk serve --config ui/Trunk.toml
```

Navigate to **http://localhost:8003** and log in through your Cognito Hosted UI.

## Configuration Reference

All API configuration is provided via environment variables.

| Variable | Required | Description |
|----------|----------|-------------|
| `DATABASE_URL` | Always | PostgreSQL connection string |
| `COOKIE_KEY` | Always | 128 hex chars (64 bytes) for cookie encryption; must be a real random key (rejected if weak/all-identical) — `openssl rand -hex 64` |
| `COOKIE_SECURE` | Prod | Set `true` in production (HTTPS only) |
| `FRONTEND_URL` | Always | Browser-facing URL of the UI |
| `API_EXTERNAL_URL` | Always | Browser-facing URL of the API |
| `HOST` | No | Bind address (default: `0.0.0.0`) |
| `PORT` | No | Bind port (default: `3003`) |
| `RUST_LOG` | No | Log level (default: `info`) |
| `OIDC_ISSUER` | Auth | Cognito issuer URL (`https://cognito-idp.<region>.amazonaws.com/<pool-id>`) |
| `OIDC_CLIENT_ID` | Auth | Cognito app-client ID |
| `OIDC_CLIENT_SECRET` | Auth | Cognito app-client secret |
| `COGNITO_USER_POOL_ID` | Admin | User Pool ID — enables `/api/admin/*` when set (optional) |
| `COGNITO_REGION` | Admin | AWS region of the User Pool (default: `us-east-2`) |
| `AWS_ACCESS_KEY_ID` | Admin | IAM key for Cognito admin operations |
| `AWS_SECRET_ACCESS_KEY` | Admin | IAM secret for Cognito admin operations |
| `SES_FROM_EMAIL` | Optional | From-address for node invite emails via AWS SES v2; if unset, invites work but no email is sent |
| `S3_ENDPOINT` | S3 | S3-compatible endpoint URL (omit for native AWS S3) |
| `S3_BUCKET` | S3 | Bucket name |
| `S3_ACCESS_KEY` | S3 | S3 access key |
| `S3_SECRET_KEY` | S3 | S3 secret key |
| `S3_REGION` | No | S3 region (default: `us-east-1`) |

---

## Cargo Build & Check Commands

```bash
# Backend + common (host target)
cargo check
cargo clippy -- -D warnings
cargo test

# WASM UI
cargo check -p ui --target wasm32-unknown-unknown
cargo clippy -p ui --target wasm32-unknown-unknown -- -D warnings
```

---

## Development Workflow & Standards

Ember Trove follows a **zero-panic, TDD-first, plan-before-you-change** workflow. The
full policy lives in [`.claude/POLICY.md`](.claude/POLICY.md); contributor mechanics in
[`CONTRIBUTING.md`](CONTRIBUTING.md); agent guardrails in [`CLAUDE.md`](CLAUDE.md).

**One-time setup — install the git hooks:**

```bash
make hooks-install      # pre-commit: cargo fmt --all --check + clippy; pre-push: tests
```

**Definition of done** (enforced by hooks locally and by CI on every push/PR):

```bash
make verify             # or ./scripts/verify.sh — runs the full suite below
#   cargo fmt --all --check          (formatting is enforced; edition 2024)
#   cargo clippy ... -- -D warnings  (api+common, and ui for wasm32)
#   cargo test --workspace --exclude ui
#   cargo check -p ui --target wasm32-unknown-unknown
```

- **Formatting:** default `rustfmt`, edition 2024. Configure your editor to format with
  `--edition 2024` so it matches the CI gate.
- **E2E smoke tests:** `./scripts/e2e.sh` runs the Playwright suite against a dedicated
  Docker stack (auth bypassed via the `e2e-bypass` cargo feature, ephemeral DB; no local
  Node required — Playwright runs in its official image). Also a CI job. See
  [`e2e/README.md`](e2e/README.md).
- **Coverage:** `make coverage` (CI reports it via `cargo llvm-cov`; currently report-only).
- **Dependencies:** [Dependabot](.github/dependabot.yml) proposes weekly `cargo` and
  `github-actions` updates against `develop`; Actions are pinned to commit SHAs.
- **Git Flow:** branch from `develop`; never commit directly to `main`/`develop`. See
  [`CONTRIBUTING.md`](CONTRIBUTING.md) for the feature/release/hotfix flow.

---

## API Reference

All routes are nested under `/api`. Interactive docs at `/swagger-ui/` when the API is running.

### Auth (public)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/auth/login` | Redirect to identity provider login |
| GET | `/api/auth/callback` | OIDC code exchange; sets session cookie |
| POST | `/api/auth/refresh` | Silent token refresh |

### Auth (protected)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/auth/me` | Current user info and roles |
| POST | `/api/auth/logout` | Clear session cookies + redirect through IdP end-session endpoint |
| POST | `/api/auth/change-password` | Change the signed-in user's Cognito password |

### Health

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Service health + database connectivity |

### Nodes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/nodes` | List nodes (status, tag_id, tag_ids, pinned, pagination) |
| POST | `/api/nodes` | Create node (auto-grants Owner permission to creator) |
| GET | `/api/nodes/{id}` | Get node by UUID |
| GET | `/api/nodes/slug/{slug}` | Get node by slug |
| PUT | `/api/nodes/{id}` | Update node (snapshots version on save) |
| DELETE | `/api/nodes/{id}` | Delete node (cascading) |
| PUT | `/api/nodes/{id}/pin` | Toggle pinned state (owner-only) |
| GET | `/api/nodes/{id}/neighbors` | Linked neighbour nodes |
| GET | `/api/nodes/{id}/backlinks` | Nodes that link to this node |
| GET | `/api/nodes/{id}/edges` | All edges involving this node |
| GET | `/api/nodes/{id}/tags` | Tags attached to this node |
| POST | `/api/nodes/{id}/tags/{tag_id}` | Attach a tag |
| DELETE | `/api/nodes/{id}/tags/{tag_id}` | Detach a tag |
| GET | `/api/nodes/{id}/attachments` | List attachments |
| POST | `/api/nodes/{id}/attachments` | Upload attachment (multipart/form-data) |
| GET | `/api/nodes/{id}/permissions` | List permission grants (viewer+) |
| POST | `/api/nodes/{id}/permissions` | Grant permission to a user |
| POST | `/api/nodes/{id}/invite` | Invite user by email (owner-only); sends SES email |
| GET | `/api/nodes/{id}/activity` | Audit log for this node |
| GET | `/api/nodes/{id}/versions` | Version history (body snapshots) |
| POST | `/api/nodes/{id}/versions/{vid}/restore` | Restore a past version |
| POST | `/api/nodes/{id}/share` | Create a public share token |
| GET | `/api/nodes/{id}/share` | List share tokens for this node |

### Edges

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/edges` | List all edges |
| POST | `/api/edges` | Create edge |
| DELETE | `/api/edges/{id}` | Delete edge |

### Tags

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/tags` | List all tags |
| POST | `/api/tags` | Create tag |
| PUT | `/api/tags/{id}` | Update tag |
| DELETE | `/api/tags/{id}` | Delete tag |

### Search

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/search?q=…` | Full-text + fuzzy search across nodes, notes, and tasks |
| GET | `/api/search?q=…&status=published` | Filter by node status |
| GET | `/api/search?q=…&tag_ids={uuid,uuid}` | Filter by tags (OR mode) |
| GET | `/api/search?q=…&tag_ids={uuid,uuid}&and_mode=true` | Filter by tags (AND mode) |
| GET | `/api/search?q=…&fuzzy=true` | Force fuzzy (trigram) matching |

### Search Presets

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/search-presets` | List saved search presets (owner-scoped) |
| POST | `/api/search-presets` | Save a search preset |
| DELETE | `/api/search-presets/{id}` | Delete a search preset |

### Attachments

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/attachments/{id}/download` | Stream attachment bytes from S3 |
| DELETE | `/api/attachments/{id}` | Delete attachment |

### Graph

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/graph/positions` | List saved node positions |
| PUT | `/api/graph/positions/{node_id}` | Save / update a node position |

### Tasks

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/nodes/{id}/tasks` | List tasks for a node |
| POST | `/api/nodes/{id}/tasks` | Create task |
| PUT | `/api/tasks/{id}` | Update task (toggle, rename, set focus date) |
| DELETE | `/api/tasks/{id}` | Delete task (soft delete — restorable for 30 days) |
| POST | `/api/tasks/{id}/restore` | Un-delete a task (backs the UI's undo toast) |
| GET | `/api/tasks/my-day` | Tasks scheduled for today (current user) |

### Notes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/nodes/{id}/notes` | List notes for a node (newest first) |
| POST | `/api/nodes/{id}/notes` | Append a note |
| PATCH | `/api/notes/{id}` | Edit a note body (owner-only) |
| DELETE | `/api/notes/{id}` | Delete a note (soft delete — restorable for 30 days) |
| POST | `/api/notes/{id}/restore` | Un-delete a note (backs the UI's undo toast) |
| GET | `/api/notes/feed` | Global notes feed (all accessible nodes, newest first) |

### Permissions (standalone)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/permissions` | List all grants for the caller (optionally filtered by `?node_id=`) |
| PUT | `/api/permissions/{id}` | Update role on an existing grant |
| DELETE | `/api/permissions/{id}` | Revoke a grant |

### Templates

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/templates` | List node templates (owner-scoped) |
| POST | `/api/templates` | Create a template |
| PUT | `/api/templates/{id}` | Update a template |
| DELETE | `/api/templates/{id}` | Delete a template |

### Public Share Links

| Method | Path | Description |
|--------|------|-------------|
| GET | `/share/{token}` | Read-only public view — no login required |
| DELETE | `/api/share/{token}` | Revoke a share token |

### Admin (admin role required)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/admin/users` | List users |
| POST | `/api/admin/users` | Create user |
| DELETE | `/api/admin/users/{id}` | Delete user |
| GET | `/api/admin/users/roles` | List available roles |
| PUT | `/api/admin/users/{id}/roles` | Set roles for a user |
| GET | `/api/admin/backup` | Stream full-system backup (NDJSON) |
| POST | `/api/admin/restore` | Restore from backup file |
| GET | `/api/metrics` | Operational metrics snapshot (version, uptime, pool stats, row counts) |

---

## Domain Model

### Node Types

| Type | Description |
|------|-------------|
| `article` | Blog post, essay, or atomic note |
| `project` | Active initiative with tasks and references |
| `area` | Sphere of responsibility (ongoing) |
| `resource` | Reference material, bookmark, or asset |
| `reference` | Citation, paper, or external source |

### Edge Types

| Type | Meaning | Graph style |
|------|---------|-------------|
| `references` | Node A cites / links to Node B | Amber, solid |
| `contains` | Node A structurally contains Node B | Green, solid, thicker |
| `related_to` | Bidirectional semantic relationship | Purple, dashed |
| `depends_on` | Node A requires Node B | Orange, dotted |
| `derived_from` | Node A was derived from Node B | Pink, long-dash |
| `wiki_link` | Auto-created from `[[title]]` syntax in body | Blue, short-dash |

### Node Statuses

| Status | Meaning |
|--------|---------|
| `draft` | Work in progress, not yet published |
| `published` | Visible in published-only search/filter |
| `archived` | Hidden from default list; accessible by direct link |

### Permission Roles

| Role | Permissions |
|------|-------------|
| `viewer` | Read node and all sub-resources |
| `editor` | Read + write node, notes, tasks, tags, edges |
| `owner` | Full access including invite, pin, delete, and share |

Nodes are **private by default** — only the creating user can see a node until they explicitly grant access. The `POST /api/nodes/{id}/invite` endpoint looks up or creates a Cognito account for the invited email and grants the specified role.

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `n` | Open quick-capture modal (new node) |
| `g` | Go to graph view |
| `/` | Focus search |
| `p` | Toggle pin on the open node |
| `?` | Toggle keyboard shortcuts help overlay |
| `Esc` | Back to node list / close modal |

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md).

## License

MIT — see [LICENSE](LICENSE).
