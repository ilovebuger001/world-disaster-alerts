# World Disaster Alerts — Rust backend + map frontend

Implementation of the **World Disaster Alerts Rev.2.9 Final Engineering Specification**,
following the specification's own implementation order (§16). This is an
implementation of the first phases — **not** a production-qualified release.

## Phase status (spec §16)

| Phase | Status | Evidence |
|---|---|---|
| 1 — Reliable core | ✅ implemented | `tests/cap_lifecycle.rs`, `tests/storage_recovery.rs`, CI live smoke test |
| 2 — Realtime and geography | ✅ implemented | `tests/antimeridian.rs`, `tests/sse_replay.rs`, `tests/api.rs` |
| 3 — Frontend and offline mode | ✅ implemented (browser-tested against a mock API; physical-device qualification §89 not done) | `frontend/` |
| 4 — AI enrichment | ⛔ not started (needs an AI provider + keys; spec says add last) | API returns `translations.status = "unavailable"` |
| 5 — Production hardening | ⛔ not started (reverse proxy/HTTPS, metrics dashboards, backup/restore drills, load/soak tests, runbooks) | `Dockerfile` only |

Exit conditions:
* **Phase 1** — CAP updates/cancellations and USGS updates are idempotent, persisted and correct after restart: `storage_recovery::lifecycle_survives_restart_and_is_idempotent`.
* **Phase 2** — a client can disconnect, reconnect, replay missing events, or receive a reset and resynchronize: `sse_replay::*`.

Not yet implemented from later revisions: official facilities domain (§70–74), critical-zone
notification & Web Push (§79–80), Web Audio engine (§81), incident grouping (§63.4),
jurisdiction/authority matching (§77), metrics/OpenTelemetry (§86). No CAP digital
signature verification is performed, and the API never claims it.

## What runs

One Rust binary (Tokio + Axum) with one embedded `redb` database:

* **Ingestion** — source registry (§4.1/§4.6), bounded polling with jitter, ETag /
  If-Modified-Since, 429 Retry-After, exponential backoff, permanent-error handling,
  streamed body size limits, redirect allow-list, SSRF/DNS-rebinding control via a
  connection-time resolver that rejects private/loopback/link-local/metadata addresses.
* **Adapters** — CAP 1.2 (streaming `quick-xml`, namespace-checked, DTD rejected,
  depth/element/text/vertex limits, multiple info/area/polygon/circle/geocode) and
  USGS ComCat GeoJSON (RFC 7946 order, preferred-id changes, `deleted` → withdrawn).
* **Lifecycle** — Actual+Public only; Test/Exercise/Draft/System/Restricted/Private/Ack/Error
  never become public markers; exact `(sender, identifier, sent)` references; history kept;
  out-of-order and duplicate messages never roll state back.
* **Single writer** — one storage thread commits alert revisions, message history,
  references, source checkpoint and event-log sequence in one transaction, then updates
  the in-memory index, then publishes (persist → index → publish).
* **Geography** — named `GeoPoint`, R\*-tree candidates + exact tests, antimeridian
  splitting, geodesic circle checks, interior points, Web Mercator grid clustering
  (≤ 50 visible features, mixed priorities exposed), paginated list independent of the map cap.
* **API** — `/health/live`, `/health/ready`, `/api/v1/meta`, `/api/v1/sources`,
  `/api/v1/alerts` (viewport + list), `/api/v1/alerts/{id}`, `/api/v1/search`,
  `/api/v1/stream` (SSE with `after`/`Last-Event-ID`, durable replay, lag recovery,
  `stream.reset`, bounded per-client queues, connection limits).
* **Frontend** — map-first SPA (Google Maps raster, dynamic import), list alternative,
  left panel / bottom sheet, original wording always shown, source links, freshness,
  search (areas + disasters), filters, TH/EN catalogs, themes, service-worker app shell,
  IndexedDB last-known snapshot labelled as possibly stale, device-local proximity sort.

## Run locally

```bash
cd backend
cargo run --release            # http://localhost:8080
```

`config/app.toml` starts in `development`. The included USGS source is enabled for
development only: its rights review is still `review_pending`, so the API reports
`public_redistribution: false`, and `environment = "production"` refuses to start until
the source passes the §4.6 onboarding reviews.

Google Maps: edit `frontend/assets/config.js` and add your server's domain to the key's
HTTP-referrer restrictions.

## Deploy (one Linux VM, spec §13)

```bash
docker build -t wda backend
docker run -d -p 8080:8080 -v wda-data:/app/data --name wda wda
```

Put HTTPS and a reverse proxy in front (with buffering disabled for `/api/v1/stream`).
GitHub Pages cannot run this server; it only hosts static files.

## Tests

`cargo test` — 39 tests covering CAP lifecycle, coordinate order, antimeridian,
clustering caps, storage recovery, event-log retention, SSE replay/reset, ingestion
failure matrix (304/429/5xx/404/oversize/redirect/timeout/DNS/private IPs) and the API.
CI also runs a live smoke test against the real USGS feed.
