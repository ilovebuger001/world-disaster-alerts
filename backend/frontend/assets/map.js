// MapLibre GL JS adapter (2D only) on OpenFreeMap vector tiles (OpenStreetMap data).
// No API key. Tiles are fetched on demand by the browser and never bulk-cached.
// Disaster markers are teardrop pins; facility markers are rounded squares with
// a ground circle at close zoom, so the two are never confused.

import { PRC, FAC, ORDER, GLYPH, glyph } from "./icons.js";
export { PRC, FAC, glyph };
const ML_VER = "5.24.0";
const ML_JS = `https://unpkg.com/maplibre-gl@${ML_VER}/dist/maplibre-gl.js`;
const ML_CSS = `https://unpkg.com/maplibre-gl@${ML_VER}/dist/maplibre-gl.css`;
// Subresource Integrity for the pinned files (CI recomputes and prints these).
const ML_JS_SRI = "sha384-5+cfbwT0iiub6VsQAdn6yz16nr6sDiQoHx6tm4O8OVYXHYOxcffFmCJBL0dgdvGp";
const ML_CSS_SRI = "sha384-uTttxo/aOKbdE5RlD/SPzSDoDmNvGlUYPjONi2MN/b7c9HPSvW07OIuyP7uL6jxK";
const RTL_PLUGIN = "https://unpkg.com/@mapbox/mapbox-gl-rtl-text@0.3.0/dist/mapbox-gl-rtl-text.js";
const STYLE = { light: "https://tiles.openfreemap.org/styles/liberty", dark: "https://tiles.openfreemap.org/styles/dark" };

function loadCss(href, integrity) {
  if (document.querySelector(`link[href="${href}"]`)) return;
  const l = document.createElement("link");
  l.rel = "stylesheet";
  l.href = href;
  l.integrity = integrity;
  l.crossOrigin = "anonymous";
  document.head.appendChild(l);
}

function loadLib() {
  if (window.maplibregl) return Promise.resolve(window.maplibregl);
  loadCss(ML_CSS, ML_CSS_SRI);
  return new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = ML_JS;
    s.integrity = ML_JS_SRI;
    s.crossOrigin = "anonymous";
    s.onload = () => (window.maplibregl ? resolve(window.maplibregl) : reject(new Error("lib")));
    s.onerror = () => reject(new Error("lib"));
    document.head.appendChild(s);
  });
}

function pinEl(hazard, pr, selected, label) {
  const c = PRC[pr] || PRC.unknown;
  const w = selected ? 40 : pr === "emergency" ? 34 : 29;
  const d = document.createElement("button");
  d.type = "button";
  d.className = "pin";
  d.setAttribute("aria-label", label);
  d.innerHTML = `<svg width="${w}" height="${Math.round(w * 1.31)}" viewBox="0 0 32 42" aria-hidden="true"><path d="M16 40.5C16 40.5 3 26.5 3 16A13 13 0 0 1 29 16C29 26.5 16 40.5 16 40.5Z" fill="${c}" stroke="#fff" stroke-width="2"/><g transform="translate(7,7) scale(.75)" color="#fff">${GLYPH[hazard] || GLYPH.other}</g></svg>`;
  return d;
}

function clusterEl(f, label) {
  const counts = f.priority_counts || {};
  const total = f.count || 1;
  let acc = 0;
  const stops = ORDER.filter((p) => counts[p]).map((p) => {
    const a = acc;
    acc += (counts[p] / total) * 360;
    return `${PRC[p]} ${a}deg ${acc}deg`;
  });
  const size = Math.min(54, 30 + Math.round(Math.log2(total) * 5));
  const d = document.createElement("button");
  d.type = "button";
  d.className = "cluster";
  d.setAttribute("aria-label", label);
  d.style.cssText = `width:${size}px;height:${size}px;background:conic-gradient(${stops.join(",") || PRC.unknown + " 0deg 360deg"})`;
  d.innerHTML = `<span style="background:${PRC[f.top_priority] || PRC.unknown}">${total}</span>`;
  return d;
}

function facilityEl(f, label) {
  const d = document.createElement("button");
  d.type = "button";
  d.className = "fac";
  d.style.background = FAC[f.status] || FAC.unverified;
  d.setAttribute("aria-label", label);
  d.innerHTML = glyph(f.kind === "hospital" || f.kind === "medical_post" ? "hospital" : f.kind === "shelter" || f.kind === "evacuation_point" ? "shelter" : "facility", 16);
  return d;
}

/** Geodesic circle as a GeoJSON ring (for CAP circles, accuracy and facility footprints). */
function circleRing(lat, lon, km, n = 48) {
  const out = [];
  const r = km / 6371.0088;
  const la = (lat * Math.PI) / 180, lo = (lon * Math.PI) / 180;
  for (let i = 0; i <= n; i++) {
    const b = (2 * Math.PI * i) / n;
    const la2 = Math.asin(Math.sin(la) * Math.cos(r) + Math.cos(la) * Math.sin(r) * Math.cos(b));
    const lo2 = lo + Math.atan2(Math.sin(b) * Math.sin(r) * Math.cos(la), Math.cos(r) - Math.sin(la) * Math.sin(la2));
    out.push([(((lo2 * 180) / Math.PI + 540) % 360) - 180, (la2 * 180) / Math.PI]);
  }
  return out;
}

const FC = (features) => ({ type: "FeatureCollection", features });
const poly = (rings, props = {}) => ({ type: "Feature", properties: props, geometry: { type: "Polygon", coordinates: rings } });

export async function createMap(el, opts) {
  const ml = await loadLib();
  try {
    if (ml.getRTLTextPluginStatus && ml.getRTLTextPluginStatus() === "unavailable") ml.setRTLTextPlugin(RTL_PLUGIN, true);
  } catch {}
  let lang = opts.lang;
  let theme = opts.theme === "dark" ? "dark" : "light";
  const map = new ml.Map({
    container: el,
    style: STYLE[theme],
    center: [100, 15],
    zoom: opts.mobile ? 1.6 : 2.4,
    minZoom: 1,
    maxZoom: 18,
    maxPitch: 0,
    pitchWithRotate: false,
    dragRotate: false,
    touchPitch: false,
    attributionControl: { compact: true },
    renderWorldCopies: true,
    cooperativeGestures: false
  });
  map.touchZoomRotate.disableRotation();
  map.keyboard.disableRotation?.();

  const data = { geom: FC([]), me: FC([]), fac: FC([]) };
  let markers = [];
  let facMarkers = [];
  let meMarker = null;

  function localizeLabels() {
    const field = ["coalesce", ["get", "name:" + lang], ["get", "name"]];
    for (const l of map.getStyle().layers || []) {
      if (l.type !== "symbol") continue;
      const tf = map.getLayoutProperty(l.id, "text-field");
      if (tf && JSON.stringify(tf).includes("name")) {
        try {
          map.setLayoutProperty(l.id, "text-field", field);
        } catch {}
      }
    }
  }

  function addOverlays() {
    for (const [id, d] of Object.entries(data)) if (!map.getSource(id)) map.addSource(id, { type: "geojson", data: d });
    const add = (layer) => !map.getLayer(layer.id) && map.addLayer(layer);
    add({ id: "geom-fill", type: "fill", source: "geom", paint: { "fill-color": ["get", "c"], "fill-opacity": 0.16 } });
    add({ id: "geom-line", type: "line", source: "geom", paint: { "line-color": ["get", "c"], "line-width": 2 } });
    add({ id: "me-fill", type: "fill", source: "me", paint: { "fill-color": "#1a73e8", "fill-opacity": 0.14 } });
    add({ id: "me-line", type: "line", source: "me", paint: { "line-color": "#1a73e8", "line-opacity": 0.5, "line-width": 1 } });
    add({ id: "fac-fill", type: "fill", source: "fac", minzoom: 14, paint: { "fill-color": ["get", "c"], "fill-opacity": 0.12 } });
    add({ id: "fac-line", type: "line", source: "fac", minzoom: 14, paint: { "line-color": ["get", "c"], "line-width": 1.5, "line-dasharray": [2, 1] } });
    localizeLabels();
  }
  map.on("style.load", addOverlays);
  const setData = (id) => map.getSource(id)?.setData(data[id]);

  let idleTimer = null;
  map.on("moveend", () => {
    clearTimeout(idleTimer);
    idleTimer = setTimeout(opts.onIdle, 250);
  });

  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("timeout")), 20000);
    map.once("load", () => (clearTimeout(t), resolve()));
    map.once("error", (e) => {
      // Style/tiles unreachable at startup: report honestly.
      if (!map.loaded()) {
        clearTimeout(t);
        reject(e.error || new Error("style"));
      }
    });
  });

  const adapter = {
    getViewport() {
      const b = map.getBounds();
      let w = b.getWest(), e = b.getEast();
      const full = e - w >= 359.9;
      const norm = (x) => ((((x + 180) % 360) + 360) % 360) - 180;
      return {
        west: full ? -180 : norm(w),
        east: full ? 180 : norm(e) === -180 ? 180 : norm(e),
        south: Math.max(-85, b.getSouth()),
        north: Math.min(85, b.getNorth()),
        zoom: map.getZoom()
      };
    },
    setFeatures(features, selectedId, labelFor) {
      for (const m of markers) m.remove();
      markers = [];
      for (const f of features) {
        const elx = f.type === "cluster" ? clusterEl(f, labelFor(f)) : pinEl(f.hazard_type, f.display_priority, f.alert_id === selectedId, labelFor(f));
        elx.style.zIndex = f.type === "cluster" ? 500 + f.count : f.alert_id === selectedId ? 9999 : 100 + (6 - ORDER.indexOf(f.display_priority));
        elx.addEventListener("click", (ev) => (ev.stopPropagation(), opts.onFeature(f)));
        markers.push(new ml.Marker({ element: elx, anchor: f.type === "cluster" ? "center" : "bottom" }).setLngLat([f.lon, f.lat]).addTo(map));
      }
    },
    setFacilities(items, labelFor) {
      for (const m of facMarkers) m.remove();
      facMarkers = [];
      data.fac = FC(items.map((f) => poly([circleRing(f.lat, f.lon, 0.06)], { c: FAC[f.status] || FAC.unverified })));
      setData("fac");
      for (const f of items) {
        const elx = facilityEl(f, labelFor(f));
        elx.addEventListener("click", (ev) => (ev.stopPropagation(), opts.onFacility && opts.onFacility(f)));
        facMarkers.push(new ml.Marker({ element: elx, anchor: "center" }).setLngLat([f.lon, f.lat]).addTo(map));
      }
    },
    showGeometry(parts, color) {
      const feats = [];
      for (const p of parts || []) {
        if (p.type === "polygon") feats.push(poly([p.exterior, ...(p.holes || [])].map((r) => r.map((q) => [q.lon_deg, q.lat_deg])), { c: color }));
        else if (p.type === "circle") feats.push(poly([circleRing(p.center.lat_deg, p.center.lon_deg, p.radius_km)], { c: color }));
      }
      data.geom = FC(feats);
      setData("geom");
    },
    clearGeometry() {
      data.geom = FC([]);
      setData("geom");
    },
    fitBBox(b) {
      if (b[0] === b[2] && b[1] === b[3]) map.easeTo({ center: [b[0], b[1]], zoom: Math.min(16, map.getZoom() + 3) });
      else map.fitBounds([[b[0], b[1]], [b[2], b[3]]], { padding: 48, maxZoom: 12 });
    },
    fitBounds(s, w, n, e) {
      map.fitBounds([[w, s], [e, n]], { padding: 32, maxZoom: 14 });
    },
    panTo(lat, lon, zoom) {
      map.easeTo({ center: [lon, lat], zoom: zoom != null ? zoom : map.getZoom() });
    },
    ensureVisible(lat, lon, minZoom) {
      map.easeTo({ center: [lon, lat], zoom: Math.max(map.getZoom(), minZoom) });
    },
    zoomBy(d) {
      map.easeTo({ zoom: map.getZoom() + d });
    },
    world() {
      map.easeTo({ center: [100, 15], zoom: 1.8 });
    },
    setTheme(t) {
      t = t === "dark" ? "dark" : "light";
      if (t === theme) return;
      theme = t;
      map.setStyle(STYLE[theme]); // overlays are re-added on style.load
    },
    setLang(l) {
      lang = l;
      if (map.isStyleLoaded()) localizeLabels();
    },
    setMe(lat, lon, acc) {
      data.me = FC([poly([circleRing(lat, lon, Math.min(acc || 100, 5000) / 1000)])]);
      setData("me");
      if (!meMarker) {
        const d = document.createElement("div");
        d.className = "me";
        meMarker = new ml.Marker({ element: d }).setLngLat([lon, lat]).addTo(map);
      } else meMarker.setLngLat([lon, lat]);
    },
    clearMe() {
      data.me = FC([]);
      setData("me");
      if (meMarker) meMarker.remove();
      meMarker = null;
    },
    resize() {
      map.resize();
    }
  };
  return adapter;
}
