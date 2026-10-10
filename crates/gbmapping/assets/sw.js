// GBMapping's service worker: the editor keeps working without a network once it has
// been opened. The network comes first (a new version is used as soon as it's there),
// the cache answers when the network doesn't. ROMs never come near it: they're read
// by the page, from the disk.
const CACHE = 'gbmapping';

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));

self.addEventListener('fetch', (event) => {
  const request = event.request;
  if (request.method !== 'GET' || new URL(request.url).origin !== self.location.origin) return;
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy)).catch(() => {});
        }
        return response;
      })
      .catch(() => caches.match(request, { ignoreSearch: true }).then((hit) => hit || Response.error()))
  );
});

// The page says which files it's made of: those are kept, older versions' are dropped
self.addEventListener('message', (event) => {
  const urls = event.data && event.data.keep;
  if (!Array.isArray(urls)) return;
  event.waitUntil(
    caches.open(CACHE).then(async (cache) => {
      await Promise.all(urls.map((url) => cache.match(url).then((hit) => hit || cache.add(url).catch(() => {}))));
      const kept = new Set(urls);
      for (const request of await cache.keys()) if (!kept.has(request.url)) await cache.delete(request);
    })
  );
});
