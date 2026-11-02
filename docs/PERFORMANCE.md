# Performance baseline (P0, 2026-11-02)

The owner asked for a performance phase once the app was close to feature-complete.
This report is its first step. It measures before changing anything, and ends in a
ranked list of fixes for the owner to approve (P1).

It starts from v3.8.1, which already fixed the worst case. The WASM was 35.6 MB and
served uncompressed, so an iPhone waited about 10 minutes after a release. It is now
2.3 MB on the wire, and an iPhone reaches the login page in 3.5 s.

## How it was measured

| What | Tool | Where |
|---|---|---|
| Page loads and actions | `e2e/perf/pages.perf.ts` (Playwright) | local stack, synthetic data |
| Statements | `pg_stat_statements` via `scripts/perf/pg-top.sh` | local stack (prod after this release) |
| Graph layout | `common/tests/layout_timings.rs` (ignored test, release mode) | native |
| WASM contents | name section + code section sizes, per crate | `wasm-release` build |
| Per-route API latency | `GET /api/metrics` → `routes` (new in this change) | prod, from this release on |

**Data.** `scripts/perf/seed.sh <scale>` writes synthetic, prod-shaped data into the
local stack only; no prod data is copied.

| Scale | Nodes | Tasks | Notes | Versions | Edges |
|---|---|---|---|---|---|
| 1× (prod's counts, 2026-11-02) | 300 | 270 | 500 | 2,100 | 183 |
| 10× (growth headroom) | 3,000 | 2,700 | 5,000 | 21,000 | 1,833 |

**Browser profiles.**
- `desktop`: Chromium, no throttling.
- `phone-4g`: Chromium as a Pixel 7, with 150 ms latency, 9 Mbps down and a 4× slower
  CPU.
- `iphone`: WebKit as an iPhone 15. WebKit can't be throttled, so this profile shows
  the engine, not the network.

**What the table columns mean.** All times are ms from the start of navigation.
- `mounted`: the app is in the page.
- `ready`: the page's content is visible.
- `data`: the last API response of the load has arrived. The notes, search and node
  pages include a 300 ms debounce in this number.
- `api`: API requests made during the load.
- `kB`: API bytes received.
- `cold` is a first visit. `warm` is a second visit in the same browser, with the
  service worker and cache in play.

## Results

### 1× (prod-sized)

| page | desktop ready (cold/warm) | phone-4g ready (cold/warm) | iPhone ready (cold) | api | kB |
|---|---|---|---|---|---|
| My Day | 174 / 130 | 3,484 / 1,110 | 585 | 10 | 73 |
| Captures | 129 / 122 | 3,467 / 607 | 188 | 8 | 10 |
| Calendar | 129 / 121 | 3,464 / 1,108 | 430 | 9 | 29 |
| Dashboard | 155 / 128 | 3,494 / 1,125 | 271 | 9 | 71 |
| Nodes | 230 / 128 | 4,544 / 1,149 | 222 | 9 | 257 |
| Notes | 133 / 123 | 3,475 / 1,107 | 435 | 9 | 54 |
| Node page | 136 / 130 | 3,491 / 1,117 | 405 | 16 | 16 |
| **Graph** | 382 / 131 | 5,473 / 3,112 | 278 | 11 | **1,558** |

### 10×

| page | desktop ready (cold/warm) | phone-4g ready (cold/warm) | iPhone ready (cold) | api | kB |
|---|---|---|---|---|---|
| **My Day** | 499 / 416 | 3,779 / 1,150 | **1,862** | 10 | **735** |
| Calendar | 151 / 135 | 4,478 / 1,108 | 202 | 9 | 293 |
| Nodes | 192 / 143 | 4,556 / 1,135 | 446 | 9 | 398 |
| Node page | 133 / 131 | 4,492 / 1,119 | 432 | 16 | 105 |
| **Graph** | 1,107 / 788 | **22,732 / 19,726** | 1,139 | 24 | **15,743** |

### Actions

| action | 1× | 10× |
|---|---|---|
| tick a task on My Day (phone-4g) | 48 ms | **963 ms** |
| quick capture on My Day | 3 requests, 64 kB refetched | 3 requests, **637 kB** refetched |
| drag a graph node, 60 mouse moves (desktop) | 1,847 ms (31 ms per move) | 1,005 ms (17 ms per move) |

### Start-up

- The phone's cold start mounts at about 2.8 s at every scale: the 2.3 MB download,
  then compiling on a slow CPU.
- Warm, it mounts at 0.26 s. Sign-in resolves 0.16 s later, and the page's data
  follows about 0.7 s after that.
- At 150 ms per round trip, that's serial requests. A page sends 8–16 API requests,
  and the proxy speaks HTTP/1.1, so the browser can only run 6 at once.
- Duplicate requests at start:
  - `/api/health` 3×;
  - `/templates` 2× (from two closed dialogs);
  - `/editor-prefs` 2× on a node page.

### Database (10×)

- **Not a bottleneck.** The slowest statement averages 6 ms and peaks at 8 ms. Nothing
  read from disk (`blks_read` 0).
- The heaviest statement in total is the node list (`GET /nodes`): 105 calls × 6 ms.
  The graph pages through every node 200 at a time, and each page repeats an
  aggregate over all edges plus a `COUNT`.
- Every connection check-in costs one round trip (`SELECT now() <> statement_timestamp()`,
  the #235 guard).

### Graph layout (native, release)

| nodes | from scratch | all pinned (a normal load) |
|---|---|---|
| 300 | 1.5 ms | 0.7 ms |
| 1,000 | 17 ms | 4.7 ms |
| 2,000 | 60 ms | 14.5 ms |

The layout isn't the problem. Even 2–4× slower as size-optimised WASM, it stays far
under a second at realistic sizes.

### WASM contents (code bytes, `wasm-release`)

| Share | What |
|---|---|
| 60% | The Leptos framework: tachys 32%, reactive_graph 14%, leptos 4%, router 3%. Most of it is our views instantiated inside its generics. |
| 13% | The app's own code |
| 5% | Markdown: pulldown-cmark, ammonia, html5ever |
| 3% | regex: tracing-subscriber's env-filter and garde, neither needed in the browser |

Total: 6.5 MB of code and 1.1 MB of data.

## Ranked fixes (P1, for approval)

Impact is what the measurements above show; effort and risk are estimates.

| # | Fix | Impact | Effort | Risk |
|---|---|---|---|---|
| 1 | **Graph: one slim request** (id, title, kind, status, tags; no bodies). Fetch the hover preview on hover. | 15.7 MB → about 0.4 MB at 10×; phone 20 s → about 1 s | M | low |
| 2 | **Fewer round trips at start:** HTTP/2 at the proxy; drop the duplicate `/health`, `/templates` and `/editor-prefs` requests | phone warm ready 1.1 s → about 0.6 s on most pages | S | low |
| 3 | **My Day backlog:** stop sending every open task with its full row on each load (page or trim it); keyed `<For>` so a tick doesn't rebuild the list | 735 kB and 1.9 s on iPhone at 10×; tick 963 ms → under 200 ms | M | medium (#250's editor state lives in these rows) |
| 4 | **Targeted refreshes:** a capture doesn't reload task lists; one list's change reloads only that list | capture: 637 kB → 0 | S–M | low |
| 5 | **Node list:** excerpts instead of bodies, and page through all nodes (#257) | 398 kB → about 40 kB; fixes the 50-node cap | M | low |
| 6 | **API JSON compression** at the proxy, after a BREACH check | API bytes about ÷5 (everything above) | S | needs the check |
| 7 | **Cold start:** precompressed brotli, and lazy-load the graph, admin and editor (Leptos `#[lazy]`) | phone cold mount 2.8 s → maybe 1.8 s | L | medium |
| 8 | **Graph drag:** throttle the minimap to one update per frame | 17–31 ms per move → about 5 ms | S | low |
| 9 | **Hygiene, not speed:** `statement_timeout`; no regex in the WASM | safety; about 3% of the WASM | S | low |

**Not worth doing now:**
- the SQL rewrites from the survey (`lower(title)` index, the full-edges aggregate,
  fuzzy search): at 10× the database answers in under 10 ms;
- moving the graph layout to a worker: under 100 ms at 2,000 nodes.

**Still to measure on prod after this release:** per-route latency from `/api/metrics`
after a day or two of real use, and `pg_stat_statements` there.
