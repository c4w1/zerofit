// Planned load for plan items, computed by the worker and cached by the
// workout's content (editing a workout invalidates only its entry).

import { call } from "./worker/client";
import type { PlannedLoad, Workout } from "./types";

const cache = new Map<string, Promise<PlannedLoad>>();

export function plannedLoad(workout: Workout, ftp: number): Promise<PlannedLoad> {
  const key = `${ftp}|${JSON.stringify(workout)}`;
  let p = cache.get(key);
  if (!p) {
    p = call("planWorkout", [workout, ftp]);
    cache.set(key, p);
    if (cache.size > 200) cache.delete(cache.keys().next().value!);
  }
  return p;
}

export const workoutDuration = (w: Workout) => w.steps.reduce((t, s) => t + s.duration_s, 0);

/** Power profile in % FTP as [seconds, pct] points for drawing the workout shape. */
export function profilePoints(w: Workout): [number, number][] {
  const pts: [number, number][] = [];
  let t = 0;
  for (const s of w.steps) {
    const [a, b] = s.target.type === "steady" ? [s.target.pct, s.target.pct] : [s.target.from, s.target.to];
    pts.push([t, a], [t + s.duration_s, b]);
    t += s.duration_s;
  }
  return pts;
}
