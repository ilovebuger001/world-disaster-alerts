// Reviewed static interface catalogs (spec §64.8). Never AI-translated at runtime.
import { prefs } from "./store.js";

const CAT = {
  th: {
    searchPh: "ค้นหาพื้นที่หรือภัยพิบัติ",
    areas: "พื้นที่",
    disasters: "ภัยพิบัติ",
    noResult: "ไม่พบผลลัพธ์",
    hz: { earthquake: "แผ่นดินไหว", flood: "น้ำท่วม", cyclone: "พายุหมุน", storm: "พายุ", wildfire: "ไฟป่า", volcano: "ภูเขาไฟ", tsunami: "สึนามิ", heat: "ความร้อน", landslide: "ดินถล่ม", other: "อื่น ๆ" },
    st: { active: "มีผล", upcoming: "กำลังจะมีผล", expired: "หมดอายุ", cancelled: "ยกเลิก", withdrawn: "ถอนประกาศ", stale: "ข้อมูลเก่า" },
    pr: { emergency: "ฉุกเฉิน", warning: "เตือนภัย", watch: "เฝ้าระวัง", advisory: "แนะนำ", information: "ข้อมูล", unknown: "ไม่ทราบ" },
    fresh: { live: "ข้อมูลสด", partial: "ข้อมูลบางส่วน", delayed: "ล่าช้า", stale: "ขาดการอัปเดต", no_sources: "ไม่มีแหล่งข้อมูล", unknown: "ไม่ทราบสถานะ", fresh: "ปกติ", unavailable: "ใช้งานไม่ได้" },
    offline: "ออฟไลน์ · ข้อมูลล่าสุดที่เก็บไว้",
    cached: "ข้อมูลล่าสุดที่เก็บไว้ อาจไม่เป็นปัจจุบัน",
    lastSync: "ซิงก์ล่าสุด",
    upd: "อัปเดต",
    filters: "ตัวกรอง",
    fHazard: "ประเภทภัย",
    fStatus: "สถานะ",
    fTime: "ช่วงเวลา (เวลาเผยแพร่/อัปเดตจากแหล่ง)",
    tAll: "ทั้งหมด",
    t6: "6 ชม.",
    t24: "24 ชม.",
    t7: "7 วัน",
    clear: "ล้างตัวกรอง",
    list: "รายการ",
    listTitle: "ในมุมมองนี้",
    listEmpty: "ไม่มีรายการที่ตรงกับตัวกรองในมุมมองนี้ — ไม่ได้หมายความว่าพื้นที่นี้ปลอดภัย",
    more: "โหลดเพิ่ม",
    nearMe: "ใกล้ตำแหน่งของฉัน",
    km: "กม.",
    noMarker: "ไม่มีการแจ้งเตือนในมุมมองนี้ ไม่ได้แปลว่าพื้นที่ปลอดภัย",
    capped: "แสดงบนแผนที่ได้ไม่ครบ รายการด้านล่างมีครบทุกรายการ",
    close: "ปิด",
    notFound: "ไม่พบการแจ้งเตือนนี้ หรือหมดอายุแล้ว",
    agency: "หน่วยงานผู้ออก",
    hazard: "ประเภทภัย",
    display: "กลุ่มการแสดงผล",
    native: "ค่าตามแหล่งข้อมูล",
    status: "สถานะ",
    published: "เวลาเผยแพร่",
    updatedSrc: "แหล่งข้อมูลปรับปรุง",
    fetched: "ดึงข้อมูลล่าสุด",
    revision: "ฉบับที่",
    freshness: "ความสดของแหล่ง",
    expires: "หมดอายุ",
    expiresUnknown: "แหล่งข้อมูลไม่ได้ระบุ",
    area: "พื้นที่",
    original: "ข้อความต้นฉบับจากหน่วยงาน",
    translation: "คำแปล",
    trUnavailable: "ยังไม่มีคำแปลด้วยเครื่อง แสดงข้อความต้นฉบับ",
    openSrc: "เปิดแหล่งข้อมูลทางการ",
    geomOfficial: "เส้นบนแผนที่คือพื้นที่ตามที่แหล่งข้อมูลระบุ",
    geomNone: "แหล่งข้อมูลไม่ได้ระบุขอบเขตพื้นที่ จึงแสดงเป็นจุดหรือข้อความเท่านั้น",
    eqNote: "ขนาดแผ่นดินไหวบอกขนาดของเหตุการณ์ ไม่ใช่ความรุนแรงของการสั่นในแต่ละพื้นที่ ข้อมูลแค็ตตาล็อกนี้เป็นรายงานหลังเกิดเหตุ",
    notPublic: "แหล่งข้อมูลนี้ยังรอตรวจสอบสิทธิ์การเผยแพร่ซ้ำ",
    sigNote: "ไม่ได้ตรวจสอบลายเซ็นดิจิทัลของข้อความ",
    helpTitle: "ช่วยเหลือ",
    hSafe: "สำคัญ",
    hSafeT: "การไม่มีหมุดไม่ได้ยืนยันว่าพื้นที่ปลอดภัย แหล่งข้อมูลอาจล่าช้า ล่ม หรือไม่ครอบคลุม ให้ทำตามคำสั่งของหน่วยงานในพื้นที่เสมอ",
    hMarkers: "หมุด",
    hMarkersT: "ไอคอนบอกประเภทภัย สีบอกกลุ่มการแสดงผล วงกลมมีตัวเลขคือกลุ่มของหลายรายการ ซึ่งอาจมีหลายระดับปนกัน ไม่มีสีเขียว เพราะไม่มีสีใดหมายถึง “ปลอดภัย”",
    hFresh: "ความสดของข้อมูล",
    hFreshT: "สถานะบนหน้าจอคือภาพรวม แต่ละแหล่งข้อมูลมีสถานะของตัวเองด้านล่าง",
    hSources: "สถานะแหล่งข้อมูล",
    hCov: "ข้อจำกัด",
    hCovT: "ครอบคลุมเฉพาะแหล่งข้อมูลที่ตั้งค่าไว้ ไม่ครอบคลุมทั่วโลก แผนที่ Google ใช้แบบออฟไลน์ไม่ได้ ยังไม่มีชั้นสถานที่ทางการและคำแปลด้วยเครื่อง",
    lastOk: "สำเร็จล่าสุด",
    sTitle: "ตั้งค่า",
    sLang: "ภาษา",
    sTheme: "ธีม",
    auto: "อัตโนมัติ",
    light: "สว่าง",
    dark: "มืด",
    styleMap: "แผนที่",
    styleSat: "ดาวเทียม",
    locate: "ตำแหน่งของฉัน",
    locWhy: "ใช้ตำแหน่งเพื่อเรียงการแจ้งเตือนตามระยะทางในเครื่องนี้เท่านั้น ไม่ส่งออกไปที่ใด",
    locFail: "ใช้ตำแหน่งไม่ได้",
    world: "ทั้งโลก",
    help: "ช่วยเหลือ",
    mapFail: "โหลดแผนที่ไม่สำเร็จ — รายการยังใช้งานได้",
    noKey: "ยังไม่ได้ตั้งค่า Google Maps — รายการยังใช้งานได้",
    newEvent: "อัปเดตใหม่",
    ago: { s: "เมื่อสักครู่", m: "นาทีที่แล้ว", h: "ชม. ที่แล้ว", d: "วันที่แล้ว" }
  },
  en: {
    searchPh: "Search areas or disasters",
    areas: "Areas",
    disasters: "Disasters",
    noResult: "No results",
    hz: { earthquake: "Earthquake", flood: "Flood", cyclone: "Cyclone", storm: "Storm", wildfire: "Wildfire", volcano: "Volcano", tsunami: "Tsunami", heat: "Heat", landslide: "Landslide", other: "Other" },
    st: { active: "Active", upcoming: "Upcoming", expired: "Expired", cancelled: "Cancelled", withdrawn: "Withdrawn", stale: "Stale" },
    pr: { emergency: "Emergency", warning: "Warning", watch: "Watch", advisory: "Advisory", information: "Information", unknown: "Unknown" },
    fresh: { live: "Live", partial: "Partial", delayed: "Delayed", stale: "Stale", no_sources: "No sources", unknown: "Unknown", fresh: "Healthy", unavailable: "Unavailable" },
    offline: "Offline · last stored data",
    cached: "Last stored data — may be out of date",
    lastSync: "Last sync",
    upd: "Updated",
    filters: "Filters",
    fHazard: "Hazard",
    fStatus: "Status",
    fTime: "Time range (source published/updated)",
    tAll: "All",
    t6: "6 h",
    t24: "24 h",
    t7: "7 d",
    clear: "Clear filters",
    list: "List",
    listTitle: "In this view",
    listEmpty: "Nothing matches in this view — this does not mean the area is safe",
    more: "Load more",
    nearMe: "Near my location",
    km: "km",
    noMarker: "No alerts in this view. That does not mean the area is safe.",
    capped: "Not every alert fits on the map; the list below is complete",
    close: "Close",
    notFound: "This alert was not found or is no longer available",
    agency: "Issuing agency",
    hazard: "Hazard",
    display: "Display group",
    native: "Source-native value",
    status: "Status",
    published: "Published",
    updatedSrc: "Source updated",
    fetched: "Last fetched",
    revision: "Revision",
    freshness: "Source freshness",
    expires: "Expires",
    expiresUnknown: "Not stated by source",
    area: "Area",
    original: "Original agency wording",
    translation: "Translation",
    trUnavailable: "No machine translation; original wording shown",
    openSrc: "Open official source",
    geomOfficial: "The outline is the area published by the source",
    geomNone: "The source supplied no area geometry; shown as a point or text only",
    eqNote: "Magnitude describes event size, not shaking at a given place. Catalog events are post-event information.",
    notPublic: "This source is pending redistribution-rights review",
    sigNote: "Message digital signature not verified",
    helpTitle: "Help",
    hSafe: "Important",
    hSafeT: "A missing marker is never proof of safety. Feeds can be delayed, down or incomplete. Always follow local authorities.",
    hMarkers: "Markers",
    hMarkersT: "Icon = hazard, colour = display group. Numbered circles group several alerts that may have mixed levels. No green: no colour means “safe”.",
    hFresh: "Freshness",
    hFreshT: "The status pill shows overall freshness; each source has its own status below.",
    hSources: "Source status",
    hCov: "Limitations",
    hCovT: "Only configured sources; not global. Google maps do not work offline. Official facilities and machine translation are not yet available.",
    lastOk: "Last success",
    sTitle: "Settings",
    sLang: "Language",
    sTheme: "Theme",
    auto: "Auto",
    light: "Light",
    dark: "Dark",
    styleMap: "Map",
    styleSat: "Satellite",
    locate: "My location",
    locWhy: "Location is used only on this device to sort alerts by distance; it is never sent anywhere",
    locFail: "Location unavailable",
    world: "World",
    help: "Help",
    mapFail: "Map failed to load — the list still works",
    noKey: "Google Maps is not configured — the list still works",
    newEvent: "New update",
    ago: { s: "just now", m: "min ago", h: "h ago", d: "d ago" }
  }
};

export let LANG = "th";
export let T = CAT.th;

export function langPref() {
  return prefs.get("lang", "auto");
}

export function resolveLang() {
  const p = langPref();
  if (p !== "auto" && CAT[p]) return p;
  for (const l of navigator.languages || [navigator.language || "en"]) {
    const b = String(l).slice(0, 2).toLowerCase();
    if (CAT[b]) return b;
  }
  return "en";
}

export function setLang() {
  LANG = resolveLang();
  T = CAT[LANG];
  document.documentElement.lang = LANG;
}

export function fmtTime(ms) {
  if (ms == null) return "—";
  const d = new Date(ms);
  return new Intl.DateTimeFormat(LANG === "th" ? "th-TH" : "en-GB", { dateStyle: "medium", timeStyle: "short" }).format(d) + " · " + d.toISOString().slice(11, 16) + " UTC";
}

export function ago(ms) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return T.ago.s;
  if (s < 3600) return Math.floor(s / 60) + " " + T.ago.m;
  if (s < 86400) return Math.floor(s / 3600) + " " + T.ago.h;
  return Math.floor(s / 86400) + " " + T.ago.d;
}
