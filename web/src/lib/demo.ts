// Demo data: the four anonymized fixture rides from the repository plus a
// sample training week, so the whole app can be explored without a FIT file.

import type { Workout, WorkoutStep } from "./types";
import { addDays, isoDay, mondayOf } from "./format";

export const DEMO_RIDES = [
  { file: "icu_laps.fit", name: "Group ride with laps (demo)" },
  { file: "icu_intervals.fit", name: "Interval session, long day (demo)" },
  { file: "wahoo_elemnt.fit", name: "Long ride, Wahoo ELEMNT (demo)" },
  { file: "icu_short.fit", name: "Short spin, HR only (demo)" },
];

export interface PlanItem {
  id: string;
  /** `YYYY-MM-DD`. */
  date: string;
  /** Minutes after midnight. */
  start_min: number;
  workout: Workout;
}

const steady = (min: number, pct: number, kind: WorkoutStep["kind"] = "active"): WorkoutStep => ({
  duration_s: Math.round(min * 60),
  kind,
  target: { type: "steady", pct },
});
const ramp = (min: number, from: number, to: number, kind: WorkoutStep["kind"]): WorkoutStep => ({
  duration_s: Math.round(min * 60),
  kind,
  target: { type: "ramp", from, to },
});
const repeat = (n: number, steps: WorkoutStep[]) => Array.from({ length: n }, () => steps).flat();

export const SAMPLE_WORKOUTS: Record<string, Workout> = {
  sweetSpot: {
    name: "Sweet spot 2x20",
    steps: [ramp(10, 50, 75, "warmup"), steady(20, 90), steady(5, 55, "recovery"), steady(20, 90), ramp(10, 60, 40, "cooldown")],
  },
  endurance: {
    name: "Endurance 90",
    steps: [ramp(10, 50, 65, "warmup"), steady(70, 68), ramp(10, 60, 45, "cooldown")],
  },
  vo2: {
    name: "VO2max 5x4",
    steps: [ramp(15, 50, 80, "warmup"), ...repeat(5, [steady(4, 115), steady(4, 50, "recovery")]), ramp(10, 60, 40, "cooldown")],
  },
  long: {
    name: "Long ride with tempo",
    steps: [ramp(20, 50, 68, "warmup"), steady(60, 68), ...repeat(3, [steady(15, 85), steady(10, 65, "recovery")]), steady(70, 66), ramp(15, 60, 45, "cooldown")],
  },
  tempo: {
    name: "Tempo 2h",
    steps: [ramp(15, 50, 70, "warmup"), steady(90, 75), ramp(15, 65, 45, "cooldown")],
  },
};

/** A training week starting on the Monday of `today`'s week. */
export function sampleWeek(today = new Date()): PlanItem[] {
  const mon = mondayOf(today);
  const item = (dayOffset: number, startMin: number, workout: Workout, id: string): PlanItem => ({
    id: `demo-${id}`,
    date: isoDay(addDays(mon, dayOffset)),
    start_min: startMin,
    workout: structuredClone(workout),
  });
  return [
    item(1, 18 * 60, SAMPLE_WORKOUTS.sweetSpot!, "tue"),
    item(2, 7 * 60, SAMPLE_WORKOUTS.endurance!, "wed"),
    item(3, 18 * 60, SAMPLE_WORKOUTS.vo2!, "thu"),
    item(5, 9 * 60, SAMPLE_WORKOUTS.long!, "sat"),
    item(6, 9 * 60 + 30, SAMPLE_WORKOUTS.tempo!, "sun"),
  ];
}
