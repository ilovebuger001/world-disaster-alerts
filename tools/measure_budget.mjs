// Measures brotli-compressed sizes of the frontend against the performance budgets.
// Usage: node tools/measure_budget.mjs backend/frontend
import { readFileSync } from "node:fs";
import { brotliCompressSync, constants } from "node:zlib";
import { join } from "node:path";

const dir = process.argv[2] || "backend/frontend";
const br = (f) => brotliCompressSync(readFileSync(join(dir, f)), { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;
const sum = (fs) => fs.reduce((a, f) => a + br(f), 0);
const kib = (n) => (n / 1024).toFixed(1) + " KiB";

const shell = ["index.html", "assets/app.css", "assets/theme.js"];
// Before the first list render: the app core plus ONE language catalog (Thai is the larger one).
const firstJs = ["assets/app.js", "assets/api.js", "assets/store.js", "assets/i18n.js", "assets/icons.js", "assets/i18n/th.js"];
const lazy = ["assets/map.js", "assets/detail.js", "assets/unmapped.js", "assets/help.js", "assets/notify.js", "assets/i18n/en.js"];
const rows = [
  ["Shell (HTML + CSS + theme)", sum(shell), 20 * 1024],
  ["Core (shell + JS before first list render)", sum(shell) + sum(firstJs), 48 * 1024],
  ["JS before list", sum(firstJs), 12 * 1024],
  ["Lazy (map adapter, detail panel, help, notifications, other catalog)", sum(lazy), null]
];
let fail = 0;
for (const [name, size, budget] of rows) {
  const ok = budget == null || size <= budget;
  if (!ok) fail++;
  console.log(`${ok ? "OK  " : "OVER"} ${name}: ${kib(size)}${budget ? " / budget " + kib(budget) : ""}`);
}
for (const f of [...shell, ...firstJs, ...lazy]) console.log(`     ${f}: ${kib(br(f))}`);
console.log("Not counted: MapLibre GL JS (third-party CDN, loaded after the shell) and map tiles.");
process.exitCode = fail ? 1 : 0;
