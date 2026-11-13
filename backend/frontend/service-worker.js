// App-shell cache. Live alert data is never served from here: the app keeps its
// own last-known snapshot, emergency directory and guides in IndexedDB together
// with the time they were saved, and labels them as stored copies.
// Map tiles and the map library come from third parties and are not cached here.
const VERSION = "wda-shell-v4";
const SHELL = [
  "./",
  "index.html",
  "offline.html",
  "manifest.webmanifest",
  "assets/app.css",
  "assets/app.js",
  "assets/api.js",
  "assets/store.js",
  "assets/map.js",
  "assets/i18n.js",
  "assets/help.js",
  "assets/notify.js",
  "assets/theme.js",
  "assets/icons.js",
  "assets/detail.js",
  "assets/i18n/th.js",
  "assets/i18n/en.js",
  "assets/icon.svg"
];
const scope = new URL(self.registration.scope);

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
  if (e.request.method !== "GET" || url.origin !== self.location.origin) return;
  const rel = url.pathname.slice(scope.pathname.length - 1);
  if (rel.startsWith("/api/") || rel.startsWith("/health/")) return; // live data: network only
  if (e.request.mode === "navigate") {
    e.respondWith(fetch(e.request).catch(() => caches.match("index.html").then((r) => r || caches.match("offline.html"))));
    return;
  }
  // Shell assets: network first so updates arrive promptly; cache when offline.
  e.respondWith(
    fetch(e.request)
      .then((res) => {
        if (res.ok) {
          const copy = res.clone();
          caches.open(VERSION).then((c) => c.put(e.request, copy));
        }
        return res;
      })
      .catch(() => caches.match(e.request))
  );
});

self.addEventListener("notificationclick", (e) => {
  e.notification.close();
  const target = new URL((e.notification.data && e.notification.data.url) || "./", scope).href;
  e.waitUntil(
    self.clients.matchAll({ type: "window", includeUncontrolled: true }).then((list) => {
      for (const c of list) if (c.url.startsWith(scope.href)) return c.navigate(target).then((w) => w && w.focus());
      return self.clients.openWindow(target);
    })
  );
});
