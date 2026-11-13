# World Disaster Alerts

A map of official disaster alerts with an emergency-number directory for every
country and territory and official preparedness guides.

* **Backend — Rust (Axum/Tokio)** in [`backend/`](backend/): ingestion (CAP 1.2,
  USGS GeoJSON), validation, de-duplication, lifecycle (update/cancel/expiry),
  redb storage, spatial index, viewport/clustering API, search, SSE, emergency
  directory, guides, event-linked facilities, notification eligibility, Nominatim
  proxy (optional), CORS/CSP/rate limiting, health checks, structured logs.
* **Frontend — vanilla HTML/CSS/JS** in [`backend/frontend/`](backend/frontend/):
  MapLibre GL JS (2D) on OpenFreeMap vector tiles (OpenStreetMap data, no API key).
  Served by the Rust server, or published to GitHub Pages pointing at the server.

There is no sample or simulated data anywhere in the product. If no data server
is reachable, the page says so.

Information and situational awareness only — not an emergency-services or
dispatch system, and not a replacement for official warning systems such as Cell
Broadcast or sirens. The absence of an alert is never evidence that an area is
safe. Always follow local authorities.

* Deployment and costs: [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)
* Security notes and key audit: [docs/SECURITY.md](docs/SECURITY.md)
* Data (sources, emergency directory, guides, facilities): [docs/DATA.md](docs/DATA.md)
