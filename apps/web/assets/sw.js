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

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      // `reload` so a stale HTTP cache cannot put an old file in the new version.
      await cache.addAll(SHELL.map((u) => new Request(u, { cache: 'reload' })));
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
      if (hit) return hit;
      try {
        return await fetch(request);
      } catch (error) {
        if (request.mode === 'navigate') {
          const fallback = await cache.match('index.html');
          if (fallback) return fallback;
        }
        throw error;
      }
    })(),
  );
});
