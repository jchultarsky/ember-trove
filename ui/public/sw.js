// Ember Trove — Service Worker (read-only offline access)
// Strategy:
//   App shell (HTML, WASM, JS, CSS, fonts, icons) → cache-first, updated on install
//   API GET requests → network-first, cached fallback for offline reads
//   API mutations (POST/PUT/PATCH/DELETE) → network-only (no offline writes)

// Bump this cache name whenever a critical UI fix ships that must evict
// stale bundles from already-installed clients. The activate handler deletes
// every cache whose name is not CACHE_NAME, so old WASM / JS / HTML is
// discarded on the next service-worker activation.
//   v2: initial PWA + security sprint 5.
//   v3: evict pre-v2.2.2 bundles (node-editor UTF-16/UTF-8 cursor panic fix).
//   v4: /tasks/* consolidation + manifest start_url change (v2.3.0).
//   v5: Web Share Target POST /share interceptor (v2.4.0).
const CACHE_NAME = "ember-trove-v5";

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
  // text/url into Trove from another app. We translate the multipart form
  // into a /api/inbox/quick call and bounce the user to the Inbox view with
  // a success or failure marker so the SPA can show a toast.
  if (request.method === "POST" && url.pathname === "/share") {
    event.respondWith(handleShareTarget(request));
    return;
  }

  // API mutations — network only, never cache.
  if (url.pathname.startsWith("/api/") && request.method !== "GET") return;

  // API GET — network-first with cache fallback for offline reads.
  if (url.pathname.startsWith("/api/")) {
    event.respondWith(networkFirstThenCache(request));
    return;
  }

  // Static assets & app shell — cache-first, fall back to network.
  event.respondWith(cacheFirstThenNetwork(request));
});

// ── Web Share Target handler ─────────────────────────────────────────────────
// The Share Sheet posts a multipart form with the field names declared in
// manifest.json's `share_target.params` (title / text / url). We forward them
// to /api/inbox/quick (cookies travel automatically because this is same-
// origin) and 303 the browser to /tasks/inbox so the SPA boots and shows the
// new task. On failure we redirect to a route that the SPA recognises and
// shows a toast for, instead of leaving the user on a blank /share URL.
async function handleShareTarget(request) {
  try {
    const form = await request.formData();
    const title = (form.get("title") || "").toString();
    const text = (form.get("text") || "").toString();
    const url = (form.get("url") || "").toString();

    // Combine `text` and `url` into the body field — the API server then
    // coalesces them with the title (see common::inbox::coalesce_capture).
    const body = [text, url].filter((s) => s.trim().length > 0).join("\n");

    const apiResp = await fetch("/api/inbox/quick", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title, body }),
    });

    if (apiResp.status === 401) {
      // Session expired or never logged in. Send the user to the SPA root
      // so AuthGate can run the login flow; flag the failure for the toast.
      return Response.redirect("/?capture=failed&reason=auth", 303);
    }
    if (!apiResp.ok) {
      return Response.redirect(`/?capture=failed&reason=${apiResp.status}`, 303);
    }
    return Response.redirect("/tasks/inbox?captured=1", 303);
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
