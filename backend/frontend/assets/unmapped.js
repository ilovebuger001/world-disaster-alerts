// Active official alerts without a mappable area (loaded when the section is opened).
import { api } from "./api.js";
import { T } from "./i18n.js";
import { PRC, glyph } from "./icons.js";
import { esc } from "./app.js";

const $ = (id) => document.getElementById(id);

export function mount(openAlert) {
    let off = 0, timer = null;
    const load = async (reset) => {
      if (reset) off = 0;
      let b;
      try {
        b = await api.unmapped(off, $("unmappedQ").value.trim());
      } catch {
        $("unmappedList").innerHTML = `<p class="mt">${esc(T.serverDown)}</p>`;
        return;
      }
      $("unmappedCount").textContent = `(${b.total})`;
      const html = b.items.map((a) => `<button class="item" data-id="${esc(a.alert_id)}"><span class="ico" style="background:${PRC[a.display_priority] || PRC.unknown}">${glyph(a.hazard_type)}</span><span><span class="t">${esc(a.headline || T.hz[a.hazard_type] || a.hazard_type)}</span><br><span class="m">${esc((a.area_desc || []).join(" · ").slice(0, 140))}</span><br><span class="m">${esc(T.pr[a.display_priority] || "")} · ${esc(a.source_agency)}</span></span></button>`).join("");
      if (reset) $("unmappedList").innerHTML = "";
      $("unmappedList").insertAdjacentHTML("beforeend", html + (b.next_offset ? `<button class="more" id="unmappedMore">${T.more}</button>` : ""));
      $("unmappedList").querySelectorAll(".item").forEach((x) => (x.onclick = () => openAlert(x.dataset.id, true)));
      const m = $("unmappedMore");
      if (m) m.onclick = () => (m.remove(), (off = b.next_offset), load(false));
    };
    load(true);
    $("unmappedQ").addEventListener("input", () => (clearTimeout(timer), (timer = setTimeout(() => load(true), 300))));
  
}
