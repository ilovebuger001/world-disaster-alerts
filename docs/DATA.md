# Data

## Alert sources

| Source | Format | State | Notes |
|---|---|---|---|
| USGS Earthquake Hazards Program (`all_day` feed) | GeoJSON | enabled in development; `legal_review_pending` | Refused in production until the owner records the redistribution review. USGS data is generally public domain with credit. |
| CAP 1.2 template | CAP | disabled | Fill in a verified national authority feed and pass review before enabling. |

Each source records agency, jurisdiction, authority role, URL, hazards, format, update model, expected freshness, terms, redistribution status, coverage and onboarding state (`backend/config/sources.example.toml`). News and social media are never ingested as alerts. Event, issue (sent) and received times are kept separately; cancellations, updates and expiry are applied.

## Emergency directory (`backend/reference/emergency_contacts.json`)

* 249 entries — every ISO 3166-1 country and territory (list from the Debian `iso-codes` package).
* Verified (every listed number confirmed on the country's own official source, core services covered): **5** — Australia (AU), United Kingdom (GB), Japan (JP), New Zealand (NZ), United States (US).
* Partially verified: **30** — EU member states (112 confirmed by the European Commission; national police/ambulance numbers not yet reviewed), plus Canada (CA), Singapore (SG), Thailand (TH).
* Unavailable (no number verified yet; the app shows a warning and no number): **214**.
* Every number has authority, source URL, verification level and date; display text and dial string are separate; `tel:` links are generated only from a plain digit string and never dialled automatically.
* Regenerate with `python3 tools/build_contacts.py backend/reference/emergency_contacts.json` after adding verified entries. The server validates the file at startup (bad dial strings, coverage claims that don't match the contacts, or non-https sources stop it).

## Preparedness guides (`backend/reference/guides.json`)

* 11 hazards with before/during/after guidance summarized from FEMA / Ready.gov (US federal government, public domain): earthquake, tsunami, flood, tropical cyclone, thunderstorm, tornado, wildfire, volcano, landslide, extreme heat, winter storm.
* Each guide shows authority, scope (US federal guidance; general reference elsewhere), source URL and retrieval date.
* Drought: marked unavailable (no complete reviewed guidance).
* No Thai government guide has been reviewed yet; Thai users see the English official text with a note. Local authority instructions always take priority.

## Facilities (`backend/reference/facilities.json`)

* Empty on purpose: no official, event-linked facility feed has been reviewed. A facility is returned only when its record links it to the selected event (alert id, or source + source record id), with status *designated*, *confirmed available*, *listed not confirmed* or *unverified*. Nearby hospitals or shelters are never inferred.

## Map and geocoding

* Map: OpenFreeMap vector tiles and styles (OpenStreetMap data, ODbL; attribution shown on the map). Tiles are requested on demand only.
* Address search: Nominatim through the Rust server, disabled by default; when enabled it is limited to one request per second, never autocompletes, caches results and shows attribution. Coordinates and the built-in gazetteer work without it.
