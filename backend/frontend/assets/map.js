// Google Maps adapter (spec §11.3, §53): one map instance, raster 2D,
// dynamic library import, markers only for server-bounded features (≤50),
// selected-alert geometry only, no work during gestures (idle-driven).

export const PRC = { emergency: "#c5221f", warning: "#e8710a", watch: "#e3a008", advisory: "#1967d2", information: "#5f6b7a", unknown: "#9aa3ae" };
const ORDER = ["emergency", "warning", "watch", "advisory", "information", "unknown"];

export const GLYPH = {
  earthquake: '<path d="M2 12h4l2-6 4 12 3-9 2 3h5" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"/>',
  flood: '<path d="M2 15c2.5 0 2.5 2 5 2s2.5-2 5-2 2.5 2 5 2 2.5-2 5-2M2 9.5c2.5 0 2.5 2 5 2s2.5-2 5-2 2.5 2 5 2 2.5-2 5-2" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>',
  cyclone: '<circle cx="12" cy="12" r="2.6" fill="none" stroke="currentColor" stroke-width="2.2"/><path d="M4 9c2-5 9-6 13-3M20 15c-2 5-9 6-13 3" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>',
  wildfire: '<path d="M12 3c1 4 6 6 6 11a6 6 0 0 1-12 0c0-3 2-4 2-7 2 1 3 3 3 3s1-4 1-7z" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round"/>',
  volcano: '<path d="M3 20l6-10h6l6 10zM10 6l-1-3M14 6l1-3M12 6V2" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"/>',
  tsunami: '<path d="M2 18c4 0 5-10 12-10 3 0 5 2 5 4-2-1-5 0-5 3 0 2 2 3 4 3h4" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"/>',
  heat: '<path d="M12 3v11M9 14.5a3 3 0 1 0 6 0V5a3 3 0 0 0-6 0z" fill="none" stroke="currentColor" stroke-width="2.2"/>',
  landslide: '<path d="M3 20h18L8 6zM14 9l3-2M16 13l3-1" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"/>',
  storm: '<path d="M7 16a4 4 0 0 1-.5-8A5.5 5.5 0 0 1 17 8.5a3.5 3.5 0 0 1 0 7.5M12 13l-2 4h3l-2 4" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round" stroke-linecap="round"/>',
  other: '<circle cx="12" cy="12" r="3" fill="currentColor"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>'
};
export const glyph = (k, s = 18) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" aria-hidden="true">${GLYPH[k] || GLYPH.other}</svg>`;

function pinEl(hazard, pr, selected, label) {
  const c = PRC[pr] || PRC.unknown;
  const w = selected ? 40 : pr === "emergency" ? 34 : 29;
  const d = document.createElement("div");
  d.className = "pin";
  d.setAttribute("role", "button");
  d.setAttribute("aria-label", label);
  d.innerHTML = `<svg width="${w}" height="${Math.round(w * 1.31)}" viewBox="0 0 32 42" aria-hidden="true"><path d="M16 40.5C16 40.5 3 26.5 3 16A13 13 0 0 1 29 16C29 26.5 16 40.5 16 40.5Z" fill="${c}" stroke="#fff" stroke-width="2"/><g transform="translate(7,7) scale(.75)" color="#fff">${GLYPH[hazard] || GLYPH.other}</g></svg>`;
  return d;
}

function clusterEl(f, label) {
  const counts = f.priority_counts || {};
  const total = f.count || 1;
  // Conic ring shows the mix of priorities so a single colour never hides mixed levels.
  let acc = 0;
  const stops = ORDER.filter((p) => counts[p]).map((p) => {
    const a = acc;
    acc += (counts[p] / total) * 360;
    return `${PRC[p]} ${a}deg ${acc}deg`;
  });
  const size = Math.min(54, 30 + Math.round(Math.log2(total) * 5));
  const d = document.createElement("div");
  d.className = "cluster";
  d.setAttribute("role", "button");
  d.setAttribute("aria-label", label);
  d.style.cssText = `width:${size}px;height:${size}px;background:conic-gradient(${stops.join(",")});transform:translateY(50%)`;
  d.innerHTML = `<span style="background:${PRC[f.top_priority] || PRC.unknown}">${total}</span>`;
  return d;
}

function loadScript(key, lang) {
  return new Promise((resolve, reject) => {
    if (window.google && window.google.maps && window.google.maps.importLibrary) return resolve();
    const t = setTimeout(() => reject(new Error("timeout")), 12000);
    window.__wdaMapsReady = () => {
      clearTimeout(t);
      resolve();
    };
    window.gm_authFailure = () => {
      clearTimeout(t);
      reject(new Error("auth"));
      document.dispatchEvent(new CustomEvent("wda:maps-auth-failure"));
    };
    const s = document.createElement("script");
    s.src = `https://maps.googleapis.com/maps/api/js?key=${encodeURIComponent(key)}&v=weekly&loading=async&callback=__wdaMapsReady&language=${lang}`;
    s.async = true;
    s.onerror = () => {
      clearTimeout(t);
      reject(new Error("script"));
    };
    document.head.appendChild(s);
  });
}

export async function createMap(el, opts) {
  const cfg = window.WDA_CONFIG || {};
  if (!cfg.googleMapsApiKey) throw new Error("nokey");
  await loadScript(cfg.googleMapsApiKey, opts.lang);
  const { Map, Polygon, Circle, RenderingType } = await google.maps.importLibrary("maps");
  const { AdvancedMarkerElement } = await google.maps.importLibrary("marker");
  const map = new Map(el, {
    center: { lat: 15, lng: 100 },
    zoom: opts.mobile ? 2 : 3,
    minZoom: 2,
    mapId: cfg.googleMapId || "DEMO_MAP_ID",
    renderingType: RenderingType ? RenderingType.RASTER : undefined,
    disableDefaultUI: true,
    gestureHandling: "greedy",
    clickableIcons: false,
    tilt: 0,
    mapTypeId: opts.style === "satellite" ? "hybrid" : "roadmap"
  });
  let markers = [];
  let shapes = [];
  let me = [];
  let idleTimer = null;
  map.addListener("idle", () => {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(opts.onIdle, 250); // debounce: no fetch during gestures
  });

  const adapter = {
    getViewport() {
      const b = map.getBounds();
      if (!b) return null;
      const ne = b.getNorthEast();
      const sw = b.getSouthWest();
      const span = b.toSpan();
      const full = span.lng() >= 359.9;
      return {
        west: full ? -180 : sw.lng(),
        east: full ? 180 : ne.lng(),
        south: Math.max(-90, sw.lat()),
        north: Math.min(90, ne.lat()),
        zoom: map.getZoom()
      };
    },
    setFeatures(features, selectedId, labelFor) {
      for (const m of markers) m.map = null;
      markers = [];
      for (const f of features) {
        let content;
        if (f.type === "cluster") {
          content = clusterEl(f, labelFor(f));
        } else {
          content = pinEl(f.hazard_type, f.display_priority, f.alert_id === selectedId, labelFor(f));
        }
        const m = new AdvancedMarkerElement({
          map,
          position: { lat: f.lat, lng: f.lon },
          content,
          zIndex: f.type === "cluster" ? 500 + f.count : f.alert_id === selectedId ? 9999 : 100 + (6 - ORDER.indexOf(f.display_priority))
        });
        m.addListener("click", () => opts.onFeature(f));
        markers.push(m);
      }
    },
    showGeometry(parts, color) {
      adapter.clearGeometry();
      for (const p of parts || []) {
        if (p.type === "polygon") {
          shapes.push(
            new Polygon({
              map,
              paths: [p.exterior, ...(p.holes || [])].map((r) => r.map((q) => ({ lat: q.lat_deg, lng: q.lon_deg }))),
              strokeColor: color,
              strokeWeight: 2,
              fillColor: color,
              fillOpacity: 0.16,
              clickable: false,
              zIndex: 1
            })
          );
        } else if (p.type === "circle") {
          shapes.push(
            new Circle({ map, center: { lat: p.center.lat_deg, lng: p.center.lon_deg }, radius: p.radius_km * 1000, strokeColor: color, strokeWeight: 2, fillColor: color, fillOpacity: 0.14, clickable: false, zIndex: 1 })
          );
        }
      }
    },
    clearGeometry() {
      for (const s of shapes) s.setMap(null);
      shapes = [];
    },
    fitBBox(b) {
      if (b[0] === b[2] && b[1] === b[3]) {
        map.panTo({ lat: b[1], lng: b[0] });
        map.setZoom(Math.min(18, map.getZoom() + 3));
      } else {
        map.fitBounds({ west: b[0], south: b[1], east: b[2], north: b[3] }, 48);
      }
    },
    panTo(lat, lon, zoom) {
      map.panTo({ lat, lng: lon });
      if (zoom != null) map.setZoom(Math.round(zoom));
    },
    ensureVisible(lat, lon, minZoom) {
      if (map.getZoom() < minZoom) map.setZoom(minZoom);
      map.panTo({ lat, lng: lon });
    },
    zoomBy(d) {
      map.setZoom(map.getZoom() + d);
    },
    world() {
      map.setCenter({ lat: 15, lng: 100 });
      map.setZoom(2);
    },
    setStyle(s) {
      map.setMapTypeId(s === "satellite" ? "hybrid" : "roadmap");
    },
    setMe(lat, lon, acc) {
      for (const x of me) x.setMap ? x.setMap(null) : (x.map = null);
      me = [];
      me.push(new Circle({ map, center: { lat, lng: lon }, radius: Math.min(acc || 100, 5000), strokeColor: "#1a73e8", strokeOpacity: 0.4, strokeWeight: 1, fillColor: "#1a73e8", fillOpacity: 0.15, clickable: false }));
      const d = document.createElement("div");
      d.className = "me";
      me.push(new AdvancedMarkerElement({ map, position: { lat, lng: lon }, content: d, zIndex: 9000 }));
    }
  };
  return adapter;
}
