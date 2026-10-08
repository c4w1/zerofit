// App state: settings, the activity list and the training plan, kept in
// sync with IndexedDB. Components read `app` reactively (Svelte 5 runes)
// and change it only through the functions below.

import { base } from "$app/paths";
import { call } from "./worker/client";
import * as db from "./db";
import type { ActivityMeta, StoredActivity } from "./db";
import { DEMO_FTP, DEMO_RIDES, redate, sampleWeek, type PlanItem } from "./demo";
import { fitTimeToMs } from "./format";
import type { AthleteSettings } from "./types";

export const app = $state({
  ready: false,
  /** A message while background work runs (for an aria-live status region). */
  status: "",
  error: "",
  settings: db.DEFAULT_SETTINGS as AthleteSettings,
  activities: [] as ActivityMeta[],
  plan: [] as PlanItem[],
  wasmVersion: "",
  wasmLoadMs: 0,
});

const FIRST_VISIT_KEY = "visited";

/** Loads stored data; on the very first visit, loads the demo. */
export async function init(): Promise<void> {
  if (app.ready) return;
  try {
    const [settings, plan, visited, metas, ready] = await Promise.all([
      db.getKv<AthleteSettings>("settings"),
      db.getKv<PlanItem[]>("plan"),
      db.getKv<boolean>(FIRST_VISIT_KEY),
      db.activityMetas(),
      call("ready", []),
    ]);
    app.settings = { ...db.DEFAULT_SETTINGS, ...settings };
    app.plan = plan ?? [];
    app.activities = sortActivities(metas);
    app.wasmVersion = ready.version;
    app.wasmLoadMs = ready.loadMs;
    if (!visited) {
      await db.setKv(FIRST_VISIT_KEY, true);
      if (metas.length === 0) await loadDemo();
    }
  } catch (e) {
    app.error = `Could not start: ${e instanceof Error ? e.message : String(e)}`;
  } finally {
    app.ready = true;
  }
}

function sortActivities(list: ActivityMeta[]): ActivityMeta[] {
  return [...list].sort((a, b) => b.startMs - a.startMs);
}

/** Decodes and analyzes one file in the worker and stores it. */
async function analyzeAndStore(
  name: string,
  bytes: ArrayBuffer,
  demo: boolean,
  startMs?: (fileStartMs: number) => number,
): Promise<ActivityMeta> {
  const id = await db.contentId(bytes);
  // Send a copy: the original stays here to be stored.
  const copy = bytes.slice(0);
  const { activity } = await call("analyze", [copy, $state.snapshot(app.settings), false], [copy]);
  const fileStart = activity.startTime > 0 ? fitTimeToMs(activity.startTime) : Date.now();
  const stored: StoredActivity = {
    id,
    name,
    bytes,
    sport: activity.sport,
    startMs: startMs ? startMs(fileStart) : fileStart,
    summary: activity.summary,
    curveDurations: Array.from(activity.curveDurations),
    curveWatts: Array.from(activity.curveWatts),
    analysisMs: activity.analysisMs,
    addedMs: Date.now(),
    demo,
  };
  await db.putActivity(stored);
  const { bytes: _b, ...meta } = stored;
  return meta;
}

export interface UploadResult {
  name: string;
  ok: boolean;
  message: string;
  id?: string;
}

/** Analyzes and stores files one by one; returns a result per file. */
export async function addFiles(
  files: { name: string; bytes: ArrayBuffer; daysAgo?: number }[],
  demo = false,
): Promise<UploadResult[]> {
  const results: UploadResult[] = [];
  for (const [i, f] of files.entries()) {
    app.status = `Analyzing ${f.name} (${i + 1} of ${files.length})…`;
    try {
      const daysAgo = f.daysAgo;
      const meta = await analyzeAndStore(
        f.name.replace(/\.fit$/i, ""),
        f.bytes,
        demo,
        daysAgo === undefined ? undefined : (ms) => redate(ms, daysAgo),
      );
      app.activities = sortActivities([...app.activities.filter((a) => a.id !== meta.id), meta]);
      results.push({
        name: f.name,
        ok: true,
        id: meta.id,
        message: `Analyzed in ${meta.analysisMs.toFixed(0)} ms`,
      });
    } catch (e) {
      results.push({ name: f.name, ok: false, message: e instanceof Error ? e.message : String(e) });
    }
  }
  app.status = "";
  return results;
}

/** Fetches the demo rides from the site and loads the sample week. */
export async function loadDemo(): Promise<void> {
  app.status = "Loading demo rides…";
  // Use the demo rider's FTP unless the visitor has saved their own settings.
  if (!(await db.getKv("settings"))) {
    app.settings = { ...db.DEFAULT_SETTINGS, ftp: DEMO_FTP };
    await db.setKv("settings", $state.snapshot(app.settings));
  }
  const files = await Promise.all(
    DEMO_RIDES.map(async (r) => {
      const res = await fetch(`${base}/demo/${r.file}`);
      if (!res.ok) throw new Error(`could not load ${r.file}`);
      return { name: `${r.name}.fit`, bytes: await res.arrayBuffer(), daysAgo: r.daysAgo };
    }),
  );
  await addFiles(files, true);
  const week = sampleWeek();
  app.plan = [...app.plan.filter((p) => !p.id.startsWith("demo-")), ...week];
  await db.setKv("plan", $state.snapshot(app.plan));
  app.status = "";
}

/** Saves settings and re-analyzes every activity with them. */
export async function saveSettings(settings: AthleteSettings): Promise<void> {
  app.settings = settings;
  await db.setKv("settings", $state.snapshot(settings));
  const all = await db.allActivities();
  for (const [i, a] of all.entries()) {
    app.status = `Re-analyzing with new settings (${i + 1} of ${all.length})…`;
    // Keep each activity's stored date (demo rides are re-dated).
    await analyzeAndStore(a.name, a.bytes, a.demo, () => a.startMs);
  }
  app.activities = sortActivities(await db.activityMetas());
  app.status = "";
}

export async function savePlan(plan: PlanItem[]): Promise<void> {
  app.plan = plan;
  await db.setKv("plan", $state.snapshot(plan));
}

export async function removeActivity(id: string): Promise<void> {
  await db.deleteActivity(id);
  app.activities = app.activities.filter((a) => a.id !== id);
}

/** Deletes every activity, the plan and settings. */
export async function clearAllData(): Promise<void> {
  await db.clearAll();
  await db.setKv(FIRST_VISIT_KEY, true);
  app.activities = [];
  app.plan = [];
  app.settings = db.DEFAULT_SETTINGS;
}
