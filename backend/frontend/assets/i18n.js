// Interface language: auto-detected from the browser, or chosen manually.
// Only reviewed catalogs are used; unknown languages fall back to English.
// Catalogs are loaded on demand so only one language is downloaded.
import { prefs } from "./store.js";

export const LANGS = ["th", "en"];
const RTL = new Set(["ar", "he", "fa", "ur", "ps", "dv", "yi", "ku"]);
const loaded = {};
export let LANG = "en";
export let T = {};

export function langPref() {
  return prefs.get("lang", "auto");
}

export function resolveLang() {
  const p = langPref();
  if (p !== "auto" && LANGS.includes(p)) return p;
  for (const l of navigator.languages || [navigator.language || "en"]) {
    const b = String(l).slice(0, 2).toLowerCase();
    if (LANGS.includes(b)) return b;
  }
  return "en";
}

export async function setLang() {
  const l = resolveLang();
  if (!loaded[l]) loaded[l] = (await import(`./i18n/${l}.js`)).default;
  LANG = l;
  T = loaded[l];
  document.documentElement.lang = LANG;
  document.documentElement.dir = RTL.has(LANG) ? "rtl" : "ltr";
}

export function fmtTime(ms) {
  if (ms == null) return "—";
  const d = new Date(ms);
  if (isNaN(d)) return "—";
  return new Intl.DateTimeFormat(LANG === "th" ? "th-TH" : "en-GB", { dateStyle: "medium", timeStyle: "short" }).format(d) + " · " + d.toISOString().slice(11, 16) + " UTC";
}

export function ago(ms) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return T.ago.s;
  if (s < 3600) return Math.floor(s / 60) + " " + T.ago.m;
  if (s < 86400) return Math.floor(s / 3600) + " " + T.ago.h;
  return Math.floor(s / 86400) + " " + T.ago.d;
}
