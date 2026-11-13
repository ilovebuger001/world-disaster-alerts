# Security notes

## Key audit (2026-11-13)

* A Google Maps browser key (`AIzaSy…Q9E`) was committed in earlier revisions
  (`index.html`, `backend/frontend/assets/config.js`, commits `dbc8b77`, `cbe1ea5`)
  and published on GitHub Pages. Both files are removed in Rev.3 and the
  frontend no longer uses Google Maps.
* **The key remains in Git history and must be treated as exposed.** Action for
  the owner: Google Cloud Console → APIs & Services → Credentials → delete the
  key (it is no longer needed). If it must be kept, restrict it to the exact
  referrers and APIs. Rewriting public Git history does not un-expose a key.
* CI fails if a key-shaped string (`AIza…`, `sk-…`, private key blocks) appears
  in the frontend, configuration or reference data, and the Pages workflow
  checks the published site.

## Controls in the Rust server

* All upstream requests are made by the server with an SSRF-guarded resolver
  (no private/loopback destinations in production), redirect allow-lists,
  timeouts and response-size limits.
* `Content-Security-Policy` (scripts only from self and pinned unpkg MapLibre),
  `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options: DENY`,
  `Permissions-Policy`, `Cross-Origin-Opener-Policy`, optional HSTS.
* CORS only for explicitly configured https origins; API is GET-only.
* Per-IP fixed-window rate limiting on `/api/*` (client IP from the socket, or the
  right-most `X-Forwarded-For` hop only when behind the configured proxy).
* Request bodies limited to 16 KiB; input validation on every parameter; a
  consistent JSON error envelope without stack traces or paths.
* Production configuration is validated before binding a port.
* The container runs as a non-root user with a read-only root filesystem,
  `no-new-privileges` and all capabilities dropped (docker-compose.yml).

## Known gaps

* MapLibre is loaded from a CDN without Subresource Integrity yet (CI prints the
  hashes; add them or self-host the file). `style-src` allows inline styles
  because marker colours are set inline.
* No Web Push (no VAPID keys provisioned); notifications work only while the
  page is open.
* CAP digital signatures are not verified (shown as such in the UI).
