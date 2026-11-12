// API client: bounded requests, cancellation and stale-response rejection
// (spec §59 item 5, §64.4).

const base = () => (window.WDA_CONFIG && window.WDA_CONFIG.apiBase) || "";

export class ApiError extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

async function getJson(path, signal) {
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
