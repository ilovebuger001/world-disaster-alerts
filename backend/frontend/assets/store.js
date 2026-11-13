// Browser storage (spec §11.2): preferences in localStorage, last known alert
// snapshot + sync time in IndexedDB. Every access tolerates failure.

export const prefs = {
  get(k, d) {
    try {
      const v = localStorage.getItem("wda." + k);
      return v == null ? d : v;
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem("wda." + k, v);
    } catch {}
  }
};

const DB = "wda";
const STORE = "snapshots";

function open() {
  return new Promise((resolve, reject) => {
    if (!("indexedDB" in self)) return reject(new Error("no indexedDB"));
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

/** Keeps a bounded copy of the last list/map snapshot (no precise user location). */
export async function saveSnapshot(snapshot) {
  try {
    const db = await open();
    await new Promise((res, rej) => {
      const tx = db.transaction(STORE, "readwrite");
      tx.objectStore(STORE).put({ ...snapshot, items: (snapshot.items || []).slice(0, 200), saved_at_ms: Date.now() }, "last");
      tx.oncomplete = res;
      tx.onerror = () => rej(tx.error);
    });
    db.close();
  } catch {}
}

/** Generic keyed cache (emergency directory, guides) with the time it was saved. */
export async function kvPut(key, value) {
  try {
    const db = await open();
    await new Promise((res, rej) => {
      const tx = db.transaction(STORE, "readwrite");
      tx.objectStore(STORE).put({ value, saved_at_ms: Date.now() }, "kv:" + key);
      tx.oncomplete = res;
      tx.onerror = () => rej(tx.error);
    });
    db.close();
  } catch {}
}

export async function kvGet(key) {
  try {
    const db = await open();
    const v = await new Promise((res, rej) => {
      const r = db.transaction(STORE).objectStore(STORE).get("kv:" + key);
      r.onsuccess = () => res(r.result || null);
      r.onerror = () => rej(r.error);
    });
    db.close();
    return v;
  } catch {
    return null;
  }
}

export async function loadSnapshot() {
  try {
    const db = await open();
    const v = await new Promise((res, rej) => {
      const r = db.transaction(STORE).objectStore(STORE).get("last");
      r.onsuccess = () => res(r.result || null);
      r.onerror = () => rej(r.error);
    });
    db.close();
    return v;
  } catch {
    return null;
  }
}
