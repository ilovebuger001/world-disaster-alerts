// App-shell cache (spec §11.2, §55.1). API data is never cached here: the app
// stores its own last-known snapshot in IndexedDB with its sync time.
// Google Maps tiles/content are never cached (Google Maps Platform terms).
const VERSION = "wda-shell-v1";
const SHELL = [
  "/",
  "/index.html",
  "/offline.html",
  "/manifest.webmanifest",
  "/assets/app.css",
  "/assets/app.js",
  "/assets/api.js",
  "/assets/store.js",
  "/assets/map.js",
  "/assets/i18n.js",
  "/assets/config.js",
  "/assets/icon.svg"
];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(VERSION).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== VERSION).map((k) => caches.delete(k)))).then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.origin !== self.location.origin) return; // Google & others: network only
  if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/health/")) return; // live data: network only
  if (e.request.mode === "navigate") {
    e.respondWith(
      fetch(e.request).catch(() => caches.match("/index.html").then((r) => r || caches.match("/offline.html")))
    );
    return;
  }
  // Shell assets: cache first, refresh in background.
  e.respondWith(
    caches.match(e.request).then((hit) => {
      const net = fetch(e.request)
        .then((res) => {
          if (res.ok) caches.open(VERSION).then((c) => c.put(e.request, res.clone()));
          return res;
        })
        .catch(() => hit);
      return hit || net;
    })
  );
});
