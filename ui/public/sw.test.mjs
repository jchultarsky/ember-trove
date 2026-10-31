// Unit tests for the service worker's routing (#129). Run: node --test ui/public/
//
// sw.js is a classic worker script, so it is evaluated here against fakes of
// `self`, `caches` and `fetch`; each test dispatches a fetch event and checks
// what the worker answered with and what it left in the cache. (A real
// browser check isn't possible in the e2e harness: Playwright-in-Docker
// aborts a service worker's install fetches — the v2.34.0 worker fails the
// same way, so it is a harness limit, not app behaviour.)
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

const SOURCE = readFileSync(fileURLToPath(new URL('./sw.js', import.meta.url)), 'utf8');
const ORIGIN = 'https://trove.test';

function fakeResponse(body, { status = 200, type = 'basic' } = {}) {
  return {
    body, status, type, ok: status >= 200 && status < 300,
    clone() { return fakeResponse(body, { status, type }); },
  };
}

/** Load sw.js with a fake cache store and a scripted network. */
function loadWorker(network) {
  const store = new Map(); // url -> response
  const listeners = {};
  const cache = {
    async match(req) { return store.get(typeof req === 'string' ? new URL(req, ORIGIN).href : req.url); },
    async put(req, res) { store.set(typeof req === 'string' ? new URL(req, ORIGIN).href : req.url, res); },
    async addAll() {},
    async keys() { return [...store.keys()].map((url) => ({ url })); },
    async delete(req) { return store.delete(req.url); },
  };
  const calls = [];
  const sandbox = {
    self: {
      location: { origin: ORIGIN },
      addEventListener: (type, fn) => { listeners[type] = fn; },
      skipWaiting() {}, clients: { claim() {} },
    },
    caches: {
      open: async () => cache,
      match: (req) => cache.match(req),
      keys: async () => ['ember-trove-v9'],
      delete: async () => true,
    },
    fetch: async (req) => {
      const url = typeof req === 'string' ? req : req.url;
      calls.push(url);
      return network(new URL(url, ORIGIN).pathname, req);
    },
    URL, Response: class { constructor(body, init = {}) { Object.assign(this, fakeResponse(body, init)); } },
    console,
  };
  vm.runInNewContext(SOURCE, sandbox);
  async function dispatch(path, { method = 'GET', mode = 'cors' } = {}) {
    let responded;
    const request = { url: new URL(path, ORIGIN).href, method, mode };
    listeners.fetch({ request, respondWith: (p) => { responded = p; } });
    return responded ? await responded : undefined; // undefined = not intercepted
  }
  return { dispatch, store, calls };
}

const ok = (body) => fakeResponse(body);

test('a navigation goes to the network even when the shell is cached', async () => {
  let version = 'old';
  const sw = loadWorker(() => ok(`<html>${version}</html>`));
  assert.equal((await sw.dispatch('/nodes/a', { mode: 'navigate' })).body, '<html>old</html>');
  version = 'new';
  // Regression: navigations were cache-first per URL, so this stayed "old".
  assert.equal((await sw.dispatch('/nodes/a', { mode: 'navigate' })).body, '<html>new</html>');
  // Only the offline shell is kept, not a copy per visited URL.
  assert.deepEqual([...sw.store.keys()], [`${ORIGIN}/`]);
});

test('offline, a navigation falls back to the cached shell', async () => {
  let online = true;
  const sw = loadWorker(() => { if (!online) throw new TypeError('offline'); return ok('<html>shell</html>'); });
  await sw.dispatch('/', { mode: 'navigate' });
  online = false;
  assert.equal((await sw.dispatch('/tasks/my-day', { mode: 'navigate' })).body, '<html>shell</html>');
});

test('hashed assets are cache-first; other static files revalidate', async () => {
  const sw = loadWorker((path) => ok(`net:${path}`));
  await sw.dispatch('/ui-847b9072e1d6f172_bg.wasm');
  await sw.dispatch('/ui-847b9072e1d6f172_bg.wasm');
  assert.equal(sw.calls.filter((u) => u.endsWith('_bg.wasm')).length, 1, 'served from cache the second time');
  await sw.dispatch('/fonts/inter-latin.woff2');
  await sw.dispatch('/fonts/inter-latin.woff2');
  assert.equal(sw.calls.filter((u) => u.endsWith('.woff2')).length, 2, 'refreshed in the background');
});

test('auth endpoints are never cached', async () => {
  const sw = loadWorker(() => ok('{"sub":"x"}'));
  assert.equal(await sw.dispatch('/api/auth/me'), undefined, 'not intercepted');
  assert.equal(sw.store.size, 0);
});

test('logout purges every cached API response, keeping static files', async () => {
  const sw = loadWorker((path) => ok(`net:${path}`));
  await sw.dispatch('/api/nodes/1');
  await sw.dispatch('/api/tasks/my-day');
  await sw.dispatch('/fonts/inter-latin.woff2');
  assert.equal([...sw.store.keys()].filter((k) => k.includes('/api/')).length, 2);
  await sw.dispatch('/api/auth/logout', { method: 'POST' });
  assert.deepEqual([...sw.store.keys()], [`${ORIGIN}/fonts/inter-latin.woff2`]);
});

test('logout purges even when the request fails', async () => {
  let fail = false;
  const sw = loadWorker((path) => { if (fail) throw new TypeError('offline'); return ok(path); });
  await sw.dispatch('/api/nodes/1');
  fail = true;
  await assert.rejects(sw.dispatch('/api/auth/logout', { method: 'POST' }));
  assert.equal(sw.store.size, 0);
});

test('a 401 from the API purges cached data and is not cached', async () => {
  let status = 200;
  const sw = loadWorker((path) => fakeResponse(`net:${path}`, { status }));
  await sw.dispatch('/api/nodes/1');
  status = 401;
  assert.equal((await sw.dispatch('/api/nodes/2')).status, 401);
  assert.equal(sw.store.size, 0);
});

test('API mutations pass straight through', async () => {
  const sw = loadWorker(() => ok('x'));
  assert.equal(await sw.dispatch('/api/nodes', { method: 'POST' }), undefined);
});
