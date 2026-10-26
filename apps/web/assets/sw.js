// The service worker: it keeps the app's own files so the installed app opens
// with no connection. It never touches anything else: books, pictures and
// exports live in the page (OPFS / IndexedDB / blob: URLs), not here, and
// requests to other sites are left alone.
//
// The two values below (the version and the file list) are filled in by
// tools/web-precache.mjs; the version is a hash of every file, so a new build is a new cache and the old one
// is deleted when the new worker takes over.
const VERSION = '__VERSION__';
const CACHE = `mangapoint-${VERSION}`;
const SHELL = __PRECACHE__;
const KNOWN = new Set(SHELL);

// A browser refuses to show a page that came through a redirect when a service worker hands it over
// ("ERR_FAILED"), and hosts redirect: Cloudflare sends /index.html to /. So a file that arrived
// through one is kept as a fresh copy of what it led to.
async function unredirected(response) {
  if (!response.redirected) return response;
  return new Response(await response.blob(), {
    status: response.status,
    statusText: response.statusText,
    headers: response.headers,
  });
}

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      // A worker from before this fix kept the redirect for the page itself, so the app would not
      // open again until it was replaced: replace it now rather than wait for its pages to close.
      const old = await caches.match('index.html');
      if (old && old.redirected) self.skipWaiting();
      const cache = await caches.open(CACHE);
      await Promise.all(
        SHELL.map(async (u) => {
          // `reload` so a stale HTTP cache cannot put an old file in the new version.
          const response = await fetch(new Request(u, { cache: 'reload' }));
          if (!response.ok) throw new Error(`${u}: ${response.status}`);
          await cache.put(u, await unredirected(response));
        }),
      );
    })(),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key.startsWith('mangapoint-') && key !== CACHE) await caches.delete(key);
      }
      await self.clients.claim();
    })(),
  );
});

// A page that has shown "a new version is ready" asks for the switch.
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'skip-waiting') self.skipWaiting();
});

function relative(url) {
  const scope = new URL(self.registration.scope);
  if (url.origin !== scope.origin || !url.pathname.startsWith(scope.pathname)) return null;
  return url.pathname.slice(scope.pathname.length);
}

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET') return;
  const url = new URL(request.url);
  const rel = relative(url);
  if (rel === null) return; // another site, or blob: / data: (never ours)
  const key = rel === '' ? 'index.html' : rel;
  if (!KNOWN.has(key) && request.mode !== 'navigate') return;
  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      // The app opens at any address inside its scope: it is one page.
      const hit = await cache.match(request.mode === 'navigate' ? 'index.html' : key, { ignoreSearch: true });
      if (hit) return unredirected(hit);
      try {
        return await fetch(request);
      } catch (error) {
        if (request.mode === 'navigate') {
          const fallback = await cache.match('index.html');
          if (fallback) return unredirected(fallback);
        }
        throw error;
      }
    })(),
  );
});
