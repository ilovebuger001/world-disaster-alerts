// Marker colours and hazard glyphs, shared by the list, panels and map.
export const PRC = { emergency: "#c5221f", warning: "#e8710a", watch: "#b7791f", advisory: "#1967d2", information: "#5f6b7a", unknown: "#8a939e" };
export const FAC = { designated: "#0b8043", confirmed_available: "#0b8043", listed_not_confirmed: "#5f6b7a", unverified: "#8a939e" };
export const ORDER = ["emergency", "warning", "watch", "advisory", "information", "unknown"];
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
  other: '<circle cx="12" cy="12" r="3" fill="currentColor"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/>',
  shelter: '<path d="M3 11l9-7 9 7M6 9v11h12V9" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round"/>',
  hospital: '<path d="M12 5v14M5 12h14" stroke="currentColor" stroke-width="3" stroke-linecap="round"/>',
  facility: '<circle cx="12" cy="12" r="5" fill="none" stroke="currentColor" stroke-width="2.2"/>'
};
export const glyph = (k, s = 18) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" aria-hidden="true">${GLYPH[k] || GLYPH.other}</svg>`;

