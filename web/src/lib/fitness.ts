// Builds the fitness input from stored activities and asks the worker for
// CTL/ATL/TSB, the season curve, CP fits and eFTP.

import { call } from "./worker/client";
import type { ActivityMeta } from "./db";
import { dayIndex } from "./format";
import type { Fitness } from "./types";

/**
 * Load per activity: TSS from power when available, else hrTSS (a ride
 * without a power meter still counts toward fitness).
 */
export function activityLoad(a: ActivityMeta): number | null {
  return a.summary.tss ?? a.summary.hr_tss ?? null;
}

export async function computeFitness(
  activities: ActivityMeta[],
  wPrimeFallback?: number,
  endMs = Date.now(),
): Promise<Fitness> {
  const input = activities.map((a) => ({
    day: dayIndex(a.startMs),
    tss: activityLoad(a),
    curve: a.curveDurations.map((d, i) => [d, a.curveWatts[i] ?? Number.NaN] as [number, number]).filter(([, w]) => Number.isFinite(w)),
  }));
  const end = Math.max(dayIndex(endMs), ...input.map((i) => i.day));
  return call("fitness", [input, end, wPrimeFallback]);
}
