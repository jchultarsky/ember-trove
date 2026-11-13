// Ember Trove — Service Worker (read-only offline access)
// Strategy:
//   Page navigations (the HTML shell) → network-first; the cached shell only
//     when offline. It used to be cache-first, which pinned a client to the
//     release it first saw on every URL it had visited, across deploys
//     (#129: "the fix doesn't work" — an old cached bundle was running).
//   Hashed assets (Trunk's name-<16 hex> WASM/JS/CSS/SVG) → cache-first:
//     immutable by name, so a new release simply has new names.
//   Other static files (fonts, manifest, sw-agnostic icons) → stale-while-
//     revalidate: instant from cache, refreshed in the background.
//   API GET requests → network-first, cached fallback for offline reads.
//   API mutations (POST/PUT/PATCH/DELETE) → network-only (no offline writes).
//   /api/auth/* → never cached. Logging out, or any 401 from the API, purges
//     every cached API response: they hold the signed-in user's personal
//     data and must not outlive the session on a shared device (#129).

// Bump this cache name whenever a critical UI fix ships that must evict
// stale bundles from already-installed clients. The activate handler deletes
// every cache whose name is not CACHE_NAME, so old WASM / JS / HTML is
// discarded on the next service-worker activation.
//   v2: initial PWA + security sprint 5.
//   v3: evict pre-v2.2.2 bundles (node-editor UTF-16/UTF-8 cursor panic fix).
//   v4: /tasks/* consolidation + manifest start_url change (v2.3.0).
//   v5: Web Share Target POST /share interceptor (v2.4.0).
//   v6: self-hosted fonts — evict cached index.html that still references
//       Google Fonts (cross-origin fonts were never SW-cached, so icons
//       were missing offline before this).
//   v7: network-first navigations + auth/API purge (#129) — also evicts the
//       stale cached HTML every v6 client is still serving.
//   v8: Resource merged into Reference (3.0.0, #222) — evicts cached API
//       JSON that still says "resource", which the new bundle can't read.
//   v9: Data health drops the related_to_both_ways rule and adds links
//       (#228) — evicts cached reports the new bundle can't read.
//   v10: Data health drops the multiple_homes rule (#228, migration 042).
//   v11: captures (#234) — quick capture and the share target land a
//        capture, not a task; evicts cached notes and inbox JSON.
//   v12: the Inbox became Captures (#240) — the share target lands on
//        /tasks/captures; evicts the shell that still routes to the Inbox.
//   v13: the graph loads from one GET /api/graph (performance P1) — evicts
//        cached graph data in the old shapes.
//   v15: task states (S5a, #314) — task counts are open / waiting / done /
//        cancelled and a task may be "waiting"; evicts cached dashboard and
//        task JSON in the old shape.
const CACHE_NAME = "ember-trove-v16";

// Trunk's content-hashed output, e.g. /ui-847b9072e1d6f172_bg.wasm.
const HASHED_ASSET = /-[0-9a-f]{16}(_bg)?\.[a-z0-9]+$/;

// App shell resources cached on install.
// Trunk hashes WASM/JS/CSS filenames, so we cache "/" (index.html) and let
// the browser follow the hashed references from there.
const SHELL_URLS = ["/", "/manifest.json", "/favicon.svg"];

// ── Install: pre-cache app shell ─────────────────────────────────────────────
self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(SHELL_URLS))
  );
  // Activate immediately — don't wait for old tabs to close.
  self.skipWaiting();
});

// ── Activate: clean up old caches ────────────────────────────────────────────
self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((names) =>
      Promise.all(
        names
          .filter((name) => name !== CACHE_NAME)
          .map((name) => caches.delete(name))
      )
    )
  );
  // Claim all open tabs so the SW is in control without a reload.
  self.clients.claim();
});

// ── Fetch: routing strategy ──────────────────────────────────────────────────
self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle same-origin requests.
  if (url.origin !== self.location.origin) return;

  // Web Share Target — iOS / Android shells POST here when the user shares
  // text/url into Trove from another app. The worker writes nothing itself:
  // it hands the text to the app, which opens the capture box with it and
  // waits for the person to press Capture (see handleShareTarget).
  if (request.method === "POST" && url.pathname === "/share") {
    event.respondWith(handleShareTarget(request));
    return;
  }

  // Logout: let it through, then drop every cached API response.
  if (url.pathname === "/api/auth/logout") {
    event.respondWith(logoutAndPurge(request));
    return;
  }

  // The session check: never cached, and it is where the worker learns the
  // session ended or someone else signed in (see sessionCheck).
  if (url.pathname === "/api/auth/me" && request.method === "GET") {
    event.respondWith(sessionCheck(request));
    return;
  }

  // Other auth endpoints (callbacks, refresh) — never cached.
  if (url.pathname.startsWith("/api/auth/")) return;

  // Admin data (user lists, backup archives) is never kept on the device.
  if (url.pathname.startsWith("/api/admin/")) return;

  // API mutations — network only, never cache.
  if (url.pathname.startsWith("/api/") && request.method !== "GET") return;

  // API GET — network-first with cache fallback for offline reads.
  if (url.pathname.startsWith("/api/")) {
    event.respondWith(networkFirstThenCache(request));
    return;
  }

  // Page loads — always ask the server first, so a deploy is picked up.
  if (request.mode === "navigate") {
    event.respondWith(networkFirstNavigation(request));
    return;
  }

  if (HASHED_ASSET.test(url.pathname)) {
    event.respondWith(cacheFirstThenNetwork(request));
  } else {
    event.respondWith(staleWhileRevalidate(request));
  }
});

// ── Navigations: network-first ───────────────────────────────────────────────
// Every route serves the same index.html; keep the latest copy under "/" for
// offline use only.
async function networkFirstNavigation(request) {
  try {
    const response = await fetch(request);
    if (response.ok && response.type === "basic") {
      const cache = await caches.open(CACHE_NAME);
      cache.put("/", response.clone());
    }
    return response;
  } catch {
    const shell = await caches.match("/");
    if (shell) return shell;
    return new Response("Offline", { status: 503, statusText: "Service Unavailable" });
  }
}

// ── Unhashed static files: stale-while-revalidate ────────────────────────────
async function staleWhileRevalidate(request) {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  const refresh = fetch(request)
    .then((response) => {
      if (response.ok && response.type === "basic") cache.put(request, response.clone());
      return response;
    })
    .catch(() => cached || new Response("Offline", { status: 503 }));
  return cached || refresh;
}

// ── Session end: purge cached personal data ──────────────────────────────────
async function purgeApiCache() {
  const cache = await caches.open(CACHE_NAME);
  const keys = await cache.keys();
  await Promise.all(
    keys
      .filter((req) => new URL(req.url).pathname.startsWith("/api/"))
      .map((req) => cache.delete(req)),
  );
}

// SECURITY: cached API answers are one person's data. The session cookie has
// no lifetime of its own, so a session usually ends by closing the browser,
// with no logout call and no API 401 to purge on: the next person to sign in
// on this browser would have been served the last one's cached data when
// offline. So the session check purges on a 401, and whenever the person
// signed in is not the one the cache was filled for (2026-11-10).
const SUBJECT_MARKER = "/__trove/cache-subject";

async function sessionCheck(request) {
  const response = await fetch(request);
  try {
    if (response.status === 401) {
      await purgeApiCache();
      const cache = await caches.open(CACHE_NAME);
      await cache.delete(SUBJECT_MARKER);
    } else if (response.ok) {
      const sub = String((await response.clone().json()).sub || "");
      const cache = await caches.open(CACHE_NAME);
      const marker = await cache.match(SUBJECT_MARKER);
      const before = marker ? await marker.text() : null;
      if (before !== sub) {
        if (before !== null) await purgeApiCache();
        await cache.put(SUBJECT_MARKER, new Response(sub));
      }
    }
  } catch {
    // Bookkeeping must never fail the session check itself.
  }
  return response;
}

async function logoutAndPurge(request) {
  try {
    return await fetch(request);
  } finally {
    // Purge even if the request failed: the user asked to leave.
    await purgeApiCache();
  }
}

// ── Web Share Target handler ─────────────────────────────────────────────────
// The Share Sheet posts a multipart form with the field names declared in
// manifest.json's `share_target.params` (title / text / url).
//
// SECURITY: the worker must not write on a share. Any website can make a
// browser POST a form to /share; this worker would answer it, and a request
// the worker makes itself carries the session cookie, SameSite=Strict or not.
// It used to call /api/inbox/quick here, so any page could put its own text
// in a signed-in person's captures (2026-11-10). Now the text goes to the app
// in the URL fragment (never sent to the server), the app opens the capture
// box with it, and nothing is saved until the person presses Capture.
const SHARE_FIELD_MAX = 10000; // capture::MAX_LEN; the app clips to it too.

async function handleShareTarget(request) {
  try {
    const form = await request.formData();
    const field = (name) => (form.get(name) || "").toString().slice(0, SHARE_FIELD_MAX);
    const title = field("title");
    const body = [field("text"), field("url")]
      .filter((s) => s.trim().length > 0)
      .join("\n")
      .slice(0, SHARE_FIELD_MAX);
    if (title.trim().length === 0 && body.length === 0) {
      return Response.redirect("/?capture=failed&reason=empty", 303);
    }
    const shared = encodeURIComponent(JSON.stringify({ title, body }));
    return Response.redirect(`/?capture=1#shared=${shared}`, 303);
  } catch (e) {
    return Response.redirect("/?capture=failed&reason=exception", 303);
  }
}

// ── Strategies ───────────────────────────────────────────────────────────────

async function cacheFirstThenNetwork(request) {
  const cached = await caches.match(request);
  if (cached) return cached;

  try {
    const response = await fetch(request);
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }
    return response;
  } catch {
    // Offline and not in cache — return the shell index for SPA navigation.
    if (request.mode === "navigate") {
      const shell = await caches.match("/");
      if (shell) return shell;
    }
    return new Response("Offline", { status: 503, statusText: "Service Unavailable" });
  }
}

async function networkFirstThenCache(request) {
  try {
    const response = await fetch(request);
    if (response.status === 401) {
      // The session is gone: nothing cached for it may be served again.
      await purgeApiCache();
      return response;
    }
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }
    return response;
  } catch {
    const cached = await caches.match(request);
    if (cached) return cached;
    return new Response(JSON.stringify({ error: "offline" }), {
      status: 503,
      headers: { "Content-Type": "application/json" },
    });
  }
}
