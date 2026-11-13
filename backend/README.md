# World Disaster Alerts — Rust backend + map frontend

Release **Rev.3**, implementing the **World Disaster Alerts Rev.2.9 Final Engineering
Specification** (the specification keeps its own revision number; "Rev.3" is this
code release). Status is reported honestly below; see `../docs/` for deployment,
security and data coverage.

## Status

| Area | Status | Evidence |
|---|---|---|
| Reliable core (CAP/USGS/NWS ingestion, lifecycle, storage) | ✅ | `tests/cap_lifecycle.rs`, `tests/nws.rs`, `tests/storage_recovery.rs` |
| Realtime and geography (viewport, clustering, SSE replay) | ✅ | `tests/antimeridian.rs`, `tests/sse_replay.rs`, `tests/api.rs` |
| Frontend (MapLibre/OpenFreeMap), offline shell | ✅ | Playwright e2e in CI (desktop + mobile) |
| Emergency directory, guides, event-linked facilities | ✅ code; data coverage partial | `tests/reference.rs`, `../docs/DATA.md` |
| Jurisdiction matching (national issuing authority only) | ✅ | `tests/reference.rs`, `tests/nws.rs` |
| Incident grouping | ✅ conservative "possibly related" (same hazard, ≤150 km, live) | `tests/reference.rs` |
| Notification eligibility + in-page notifications + Web Audio tone | ✅ (page open only) | `tests/reference.rs`, `frontend/assets/notify.js` |
| Machine translation | ✅ code (LibreTranslate-compatible, off by default; needs a provider) | `src/api/translate.rs` |
| Web Push / background notifications | ⛔ not implemented (needs VAPID keys + crypto dependencies) | — |
| Metrics, load test, backup/restore drill, Docker verification | ✅ in CI | `.github/workflows/ci.yml`, `e2e/` |
| Public deployment | ⛔ not done (needs a server, domain and source approvals) | `../docs/DEPLOYMENT.md` |

No CAP digital signature verification is performed, and the API never claims it.

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
