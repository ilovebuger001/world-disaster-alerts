# Data coverage (reviewed 2026-11-13)

## Official alert sources

| Source | Format / adapter | Coverage | Licensing evidence | Onboarding |
|---|---|---|---|---|
| USGS Earthquake Hazards Program — `all_day` feed | GeoJSON / `usgs_geojson` | Global catalogue (completeness varies) | USGS-authored data generally public domain, credit USGS ([policy](https://www.usgs.gov/information-policies-and-instructions/copyrights-and-credits)) | enabled in development; `legal_review_pending` |
| US National Weather Service — `api.weather.gov/alerts/active` | CAP fields in GeoJSON / `nws_geojson` | US states, territories, marine zones | US Government work; API requires an identifying User-Agent ([docs](https://www.weather.gov/documentation/services-web-api)) | enabled in development; `legal_review_pending` |
| National CAP 1.2 feed (template) | CAP / `cap` | — | must be verified per authority | disabled |

Both live sources were ingested successfully by the CI smoke test against the real endpoints (USGS ~250 events/day; NWS 428 active alerts on 2026-11-13). Production mode refuses to start while any enabled source lacks an owner-recorded review — that approval has **not** been given and is not bypassed.

Candidates researched but **not** integrated (licence/terms or access not yet confirmed): MeteoAlarm (EUMETNET) CAP feeds, GDACS (EC JRC), Thai Meteorological Department / NDWC (DDPM) CAP, JMA XML feeds, PAGASA, BMKG. Each needs an owner review of terms, attribution and redistribution before enabling.

Zone-only NWS alerts (UGC/SAME codes, no polygon) are listed under “Alerts without a map area” — no boundary is invented.

## Emergency directory (`backend/reference/emergency_contacts.json`)

* Entries: **249** (every ISO 3166-1 country/territory, Debian `iso-codes`).
* Coverage: **verified 18**, **partially verified 26**, **unavailable 205**.
* Numbers: 69 verified on the responsible authority's (or an EU institution's) site, 0 from another government's official site.
* Verified countries: Australia (AU), Brazil (BR), Canada (CA), Germany (DE), France (FR), United Kingdom (GB), Hong Kong (HK), Indonesia (ID), India (IN), Japan (JP), South Korea (KR), Mexico (MX), Malaysia (MY), New Zealand (NZ), Philippines (PH), Thailand (TH), United States (US), South Africa (ZA).
* Partially verified: Austria (AT), Belgium (BE), Bulgaria (BG), Cyprus (CY), Czechia (CZ), Denmark (DK), Spain (ES), Estonia (EE), Finland (FI), Greece (GR), Croatia (HR), Hungary (HU), Ireland (IE), Italy (IT), Lithuania (LT), Luxembourg (LU), Latvia (LV), Malta (MT), Netherlands (NL), Poland (PL), Portugal (PT), Romania (RO), Singapore (SG), Slovakia (SK), Slovenia (SI), Sweden (SE).
* Unavailable countries show a warning and **no number** (“do not assume 112 or 911 works”).
* Each entry: service type, display text, dial string, authority, jurisdiction/region, network limits, source URL, verification status and date. `tel:` links are built only from plain digit strings and never dial automatically. The directory is cached on the device with its save date for offline use.

## Preparedness guides (`backend/reference/guides.json`)

* 12 guides, hazards: cyclone, earthquake, flood, heat, landslide, storm, tornado, tsunami, volcano, wildfire, winter.
* English: FEMA / Ready.gov (US federal guidance, public domain).
* Thai: flood guidance from the Department of Disaster Prevention and Mitigation (ปภ.), summarised in Thai with source link. Other Thai hazards are not yet reviewed; Thai readers get the English official text with a note.
* No machine-translated guides are published. Drought: marked unavailable.

## Facilities (`backend/reference/facilities.json`)

* **0 facilities.** No official, machine-readable feed that links shelters/hospitals to a specific alert was found and verified. The schema, validation and API are ready; nearby places are never inferred.

## Map and geocoding

* Map: OpenFreeMap vector tiles (OpenStreetMap data, ODbL; attribution on the map). MapLibre GL JS 5.24.0 from unpkg with SRI.
* Address search: Nominatim via the Rust server, off by default, ≤1 request/s, cached, attributed.
