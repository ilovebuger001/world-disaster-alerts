# Deployment

## What runs where

| Part | Where | Notes |
|---|---|---|
| Rust server (`wda-server`) | One Linux server (Docker) | Holds all logic and any secrets; persistent volume for `wda.redb` |
| Caddy | Same server | Automatic HTTPS (Let's Encrypt), SSE unbuffered |
| Frontend | Served by the Rust server at `/`, and optionally GitHub Pages | Pages is static only |
| Map tiles | OpenFreeMap (`tiles.openfreemap.org`) | Free, no key, OSM attribution shown on the map |
| Map library | unpkg CDN, pinned `maplibre-gl@5.24.0` | BSD-3-Clause |

GitHub Pages **cannot** run the backend. Publishing only to Pages without a
server gives an honest "no data server configured" page.

## One server with Docker

```sh
git clone https://github.com/ilovebuger001/world-disaster-alerts && cd world-disaster-alerts
cp backend/config/app.production.example.toml backend/config/app.production.toml
# create backend/config/sources.production.toml with ONLY sources whose review is approved
WDA_DOMAIN=alerts.example.org docker compose up -d --build
curl https://alerts.example.org/health/ready
```

The server refuses to start in production if an enabled source has not passed
review (`onboarding_state = "approved"` and an approved
`redistribution_status`, recorded by the owner after reading the source's terms), if CORS uses `*`, if rate limiting is off or if
private destinations are allowed. **Do not weaken these checks to make it start.**

### Environment overrides and secrets
`WDA_CONFIG`, `WDA_BIND`, `WDA_CORS_ORIGINS` (comma-separated https origins),
`WDA_TRUST_FORWARDED_FOR`, `WDA_HSTS`, `WDA_GEOCODER_ENABLED`, `RUST_LOG`.

Optional, server-side only (never in files, the frontend or Git):

| Variable | Purpose |
|---|---|
| `WDA_METRICS_TOKEN` (≥16 chars) | Enables `/health/metrics` in production (`Authorization: Bearer …`) |
| `WDA_TRANSLATE_URL` | LibreTranslate-compatible endpoint (e.g. `http://libretranslate:5000` in compose) |
| `WDA_TRANSLATE_API_KEY` | Only if that translation service requires a key |

Put them in a root-only `.env` next to `docker-compose.yml` and reference them under
`environment:`; do not commit the file.

## Running on Windows 11 or macOS

These are for running and testing the full system on your own computer. A public
site still needs an always-on Linux server with a domain (below).

**Option A — Docker Desktop (Windows 11 / macOS, recommended)**

1. Install Docker Desktop (Windows: enable the WSL 2 backend when asked).
2. Get the code: `git clone https://github.com/ilovebuger001/world-disaster-alerts`
   (or GitHub → Code → Download ZIP and unzip).
3. In a terminal (Windows: PowerShell; macOS: Terminal) inside the folder:
   ```sh
   docker build -t wda backend
   docker run --rm -p 8080:8080 -e WDA_BIND=0.0.0.0:8080 -v wda-data:/app/data wda
   ```
4. Open <http://localhost:8080>. Stop with Ctrl+C. Data persists in the `wda-data` volume.

**Option B — native Rust toolchain**

1. Install Rust from <https://rustup.rs> (Windows: also install "Desktop development
   with C++" from the Visual Studio Build Tools when rustup asks).
2. ```sh
   cd backend
   cargo run --release            # uses config/app.toml (development)
   ```
3. Open <http://127.0.0.1:8080>.
4. Tests: `cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings`.
   End-to-end: `pip install playwright && python -m playwright install chromium`, then
   follow the commands at the top of `backend/e2e/test_e2e.py`.

Both options run in **development** mode (unreviewed sources allowed, HTTP on
localhost). Production mode is for the Linux server with HTTPS.

## GitHub Pages (optional mirror of the frontend)

1. Settings → Pages → Build and deployment → Source: **GitHub Actions**.
2. Settings → Secrets and variables → Actions → Variables → `WDA_API_BASE` =
   `https://alerts.example.org` (the server's public HTTPS origin).
3. Add the Pages origin to `cors_allowed_origins` on the server.
4. Run the `pages` workflow (or push to `main`).

## Backups and restore

The only state is `/app/data/wda.redb` (alerts, revisions, event log, source
health). Reference data (contacts, guides, facilities) is in the repository.

```sh
# backup (consistent: stop briefly, copy, start)
docker compose stop wda
docker run --rm -v world-disaster-alerts_wda-data:/d -v "$PWD":/b debian:bookworm-slim \
  cp /d/wda.redb /b/wda-$(date +%F).redb
docker compose start wda
# restore
docker compose stop wda
docker run --rm -v world-disaster-alerts_wda-data:/d -v "$PWD":/b debian:bookworm-slim \
  cp /b/wda-YYYY-MM-DD.redb /d/wda.redb
docker compose start wda
```

Losing the database is not catastrophic: on restart the server re-ingests the
current feeds; only history older than the feeds is lost.

## Verification done in CI (every push)

fmt, 50+ Rust tests, clippy `-D warnings`, `cargo audit`, release build, secret scan,
MapLibre SRI check, Playwright e2e (desktop + mobile) against the real server, a
4,000-request load test, metrics check, a backup → wipe → restore drill, and a
Docker job that builds the production image, checks `/health/ready` and the
container health check, and confirms the production config **refuses** to start
with unreviewed sources.

## Health and logs

* `GET /health/live` — process is up. `GET /health/ready` — database open, index built.
* Container health check: `wda-server --healthcheck`.
* `GET /health/metrics` — Prometheus text (requests, errors, rate-limited, live alerts,
  SSE clients, per-source freshness/failures). Production requires `WDA_METRICS_TOKEN`.
* Logs are JSON/structured to stdout (`docker compose logs -f wda`). Query strings
  are not logged at the default `info` level.

## Cost ($0/month target)

| Item | Free option | Limits to check before relying on it |
|---|---|---|
| Server | Oracle Cloud Always Free (Ampere A1) or Google Cloud e2-micro free tier | Both require a payment card for sign-up; capacity and terms change — verify on the provider's pricing page before choosing |
| Domain + HTTPS | A subdomain you already own, or a free dynamic-DNS name; Let's Encrypt via Caddy | Let's Encrypt rate limits |
| Map tiles | OpenFreeMap public instance | No SLA ("as is"); heavy traffic should self-host OpenFreeMap or use a paid plan |
| Geocoding | Nominatim public instance (off by default) | ≤1 request/s, no autocomplete, no bulk; heavy use needs a self-hosted Nominatim or paid provider |
| CI / Pages | GitHub Actions + Pages (public repo) | Fair-use limits |

No paid service is configured. Anything paid needs the owner's approval.
