// API client: bounded requests, cancellation and stale-response rejection
// (spec §59 item 5, §64.4).

// API origin comes from <meta name="wda-api-base">: empty = same origin (served
// by the Rust server); a URL = that server; "unconfigured" = no backend known.
const META = document.querySelector('meta[name="wda-api-base"]');
const RAW = META ? META.content.trim() : "";
export const backendConfigured = () => RAW !== "unconfigured" && !RAW.startsWith("__");
const base = () => (backendConfigured() ? RAW.replace(/\/$/, "") : "");

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export async function getJson(path, signal) {
  if (!backendConfigured()) throw new ApiError(0, "unconfigured", "No data server is configured.");
  const res = await fetch(base() + path, { signal, headers: { Accept: "application/json" } });
  let body = null;
  try {
    body = await res.json();
  } catch {}
  if (!res.ok) {
    const e = (body && body.error) || {};
    throw new ApiError(res.status, e.code || "http_" + res.status, e.message || res.statusText);
  }
  return body;
}

async function staticJson(path) {
  const res = await fetch(path, { headers: { Accept: "application/json" } });
  if (!res.ok) throw new ApiError(res.status, "http_" + res.status, res.statusText);
  return res.json();
}

/** Wraps a request kind so only the newest response is ever applied. */
export function latest() {
  let ctrl = null;
  let gen = 0;
  return async (path) => {
    if (ctrl) ctrl.abort();
    ctrl = new AbortController();
    const mine = ++gen;
    const body = await getJson(path, ctrl.signal);
    if (mine !== gen) throw new DOMException("stale", "AbortError");
    return body;
  };
}

export const api = {
  meta: () => getJson("/api/v1/meta"),
  sources: () => getJson("/api/v1/sources"),
  detail: (id) => getJson("/api/v1/alerts/" + encodeURIComponent(id)),
  contact: (code) => getJson("/api/v1/contacts/" + encodeURIComponent(code)),
  unmapped: (offset, q) => getJson("/api/v1/alerts-unmapped?" + new URLSearchParams({ offset: String(offset || 0), limit: "50", ...(q ? { q } : {}) }).toString()),
  related: (id) => getJson("/api/v1/alerts/" + encodeURIComponent(id) + "/related"),
  translate: (id, lang) => getJson("/api/v1/alerts/" + encodeURIComponent(id) + "/translate?" + new URLSearchParams({ lang }).toString()),
  facilities: (id) => getJson("/api/v1/alerts/" + encodeURIComponent(id) + "/facilities"),
  // Without a data server (e.g. a static Pages mirror) the same reviewed
  // reference files are published next to the page, so the directory still works.
  contacts: () => (backendConfigured() ? getJson("/api/v1/contacts") : staticJson("reference/emergency_contacts.json")),
  guides: () => (backendConfigured() ? getJson("/api/v1/guides") : staticJson("reference/guides.json")),
  geocode: (q, lang) => getJson("/api/v1/geocode?" + new URLSearchParams({ q, lang }).toString()),
  reverse: (lat, lon) => getJson("/api/v1/geocode/reverse?" + new URLSearchParams({ lat: lat.toFixed(3), lon: lon.toFixed(3) }).toString()),
  eligible: (lat, lon) =>
    getJson("/api/v1/notify/eligible?" + new URLSearchParams({ lat: lat.toFixed(2), lon: lon.toFixed(2), radius_km: "50", min_priority: "warning" }).toString()),
  viewportPath(v, f, cursor) {
    const p = new URLSearchParams({
      west: v.west.toFixed(5),
      south: v.south.toFixed(5),
      east: v.east.toFixed(5),
      north: v.north.toFixed(5),
      zoom: String(Math.max(0, Math.min(22, Math.round(v.zoom)))),
      limit: "20"
    });
    if (f.types.size) p.set("types", [...f.types].join(","));
    if (f.statuses.size) p.set("status", [...f.statuses].join(","));
    if (f.sinceMs) p.set("since_ms", String(f.sinceMs));
    if (cursor) p.set("cursor", cursor);
    return "/api/v1/alerts?" + p.toString();
  },
  searchPath: (q) => "/api/v1/search?" + new URLSearchParams({ q, limit: "5" }).toString(),
  streamUrl: (after) => base() + "/api/v1/stream?after=" + encodeURIComponent(after)
};
