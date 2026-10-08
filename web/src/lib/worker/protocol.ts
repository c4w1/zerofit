// The messages between the UI thread and the analysis worker.
//
// Every request carries an id; the worker answers with the same id and
// either a result or an error message. Large typed arrays (FIT bytes in,
// per-second streams out) are *transferred*, not copied: ownership of the
// underlying ArrayBuffer moves to the other thread.

import type {
  AnalyzedActivity,
  AthleteSettings,
  DayPlan,
  Fitness,
  FuelDayIn,
  PlannedLoad,
  Streams,
  Workout,
} from "../types";

export interface FitnessActivityIn {
  day: number;
  tss: number | null;
  curve: [number, number][];
}

export interface Methods {
  /** Loads the WASM module; returns its version and load time. */
  ready(): { version: string; loadMs: number };
  analyze(
    bytes: ArrayBuffer,
    settings: AthleteSettings,
    withStreams: boolean,
  ): { activity: AnalyzedActivity; streams: Streams | null };
  fitness(activities: FitnessActivityIn[], endDay: number, wPrimeFallback?: number): Fitness;
  planWorkout(workout: Workout, ftp: number): PlannedLoad;
  exportZwo(workout: Workout): string;
  exportFit(workout: Workout, timeCreated: number): Uint8Array;
  fuelingDay(input: FuelDayIn): DayPlan;
}

export type Method = keyof Methods;

export interface Request<M extends Method = Method> {
  id: number;
  method: M;
  args: Parameters<Methods[M]>;
}

export type Response =
  | { id: number; ok: true; result: unknown }
  | { id: number; ok: false; error: string };
