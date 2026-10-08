// Browser-local storage (IndexedDB). Nothing here ever leaves the device:
// the app has no backend, no accounts and makes no network requests other
// than loading its own static files.
//
// Stores:
//   activities: id → StoredActivity (original FIT bytes + analysis results)
//   kv:         key → value (settings, plan, flags)

import type { ActivitySummary, AthleteSettings } from "./types";

export interface StoredActivity {
  id: string;
  name: string;
  /** Original file, kept so activities can be re-analyzed when settings change. */
  bytes: ArrayBuffer;
  sport: string | null;
  /** Start as Unix ms (from the FIT timestamp). */
  startMs: number;
  summary: ActivitySummary;
  curveDurations: number[];
  curveWatts: number[];
  analysisMs: number;
  addedMs: number;
  demo: boolean;
}

/** List-view projection: everything but the bytes. */
export type ActivityMeta = Omit<StoredActivity, "bytes">;

const DB_NAME = "zerofit";
const DB_VERSION = 1;

let dbPromise: Promise<IDBDatabase> | null = null;

function open(): Promise<IDBDatabase> {
  dbPromise ??= new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains("activities")) db.createObjectStore("activities", { keyPath: "id" });
      if (!db.objectStoreNames.contains("kv")) db.createObjectStore("kv");
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("IndexedDB unavailable"));
  });
  return dbPromise;
}

function promisify<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("IndexedDB request failed"));
  });
}

async function store(name: "activities" | "kv", mode: IDBTransactionMode) {
  const db = await open();
  return db.transaction(name, mode).objectStore(name);
}

export async function putActivity(a: StoredActivity): Promise<void> {
  await promisify((await store("activities", "readwrite")).put(a));
}

export async function getActivity(id: string): Promise<StoredActivity | undefined> {
  return promisify((await store("activities", "readonly")).get(id));
}

export async function allActivities(): Promise<StoredActivity[]> {
  return promisify((await store("activities", "readonly")).getAll());
}

/** All activities without their FIT bytes (for lists and fitness). */
export async function activityMetas(): Promise<ActivityMeta[]> {
  const all = await allActivities();
  return all.map(({ bytes: _bytes, ...meta }) => meta);
}

export async function deleteActivity(id: string): Promise<void> {
  await promisify((await store("activities", "readwrite")).delete(id));
}

export async function clearAll(): Promise<void> {
  await promisify((await store("activities", "readwrite")).clear());
  await promisify((await store("kv", "readwrite")).clear());
}

export async function getKv<T>(key: string): Promise<T | undefined> {
  return promisify((await store("kv", "readonly")).get(key));
}

export async function setKv<T>(key: string, value: T): Promise<void> {
  await promisify((await store("kv", "readwrite")).put(value, key));
}

/** A stable id from the file's contents, so re-uploading a file replaces it. */
export async function contentId(bytes: ArrayBuffer): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest).slice(0, 12), (b) => b.toString(16).padStart(2, "0")).join("");
}

export const DEFAULT_SETTINGS: AthleteSettings = {
  ftp: 250,
  weight_kg: 72,
  lthr: 165,
  max_hr: 188,
  resting_hr: 50,
  trimp: "male",
  power_zones: [0.55, 0.75, 0.9, 1.05, 1.2, 1.5],
  hr_zones: [0.81, 0.89, 0.93, 0.99, 1.02, 1.06],
};
