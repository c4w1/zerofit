/// <reference lib="webworker" />
// The analysis worker: owns the WebAssembly module and runs every Rust
// call off the UI thread, so decoding a 6-hour ride or computing its power
// curve never blocks scrolling or input.

import init, * as z from "../wasm/pkg/zerofit_wasm.js";
import wasmUrl from "../wasm/pkg/zerofit_wasm_bg.wasm?url";
import type { Method, Methods, Request, Response } from "./protocol";
import type { AnalyzedActivity, Streams } from "../types";

declare const self: DedicatedWorkerGlobalScope;

let loaded: Promise<{ version: string; loadMs: number }> | null = null;

function load() {
  loaded ??= (async () => {
    const t0 = performance.now();
    await init({ module_or_path: wasmUrl });
    return { version: z.version(), loadMs: performance.now() - t0 };
  })();
  return loaded;
}

/** Collects every typed array's buffer in `value` for transfer. */
function transferables(value: unknown): Transferable[] {
  const out: Transferable[] = [];
  const visit = (v: unknown) => {
    if (ArrayBuffer.isView(v)) {
      out.push(v.buffer as ArrayBuffer);
    } else if (v && typeof v === "object") {
      for (const x of Object.values(v)) visit(x);
    }
  };
  visit(value);
  return out;
}

const handlers: { [M in Method]: (...args: Parameters<Methods[M]>) => Promise<ReturnType<Methods[M]>> } = {
  ready: () => load(),

  async analyze(bytes, settings, withStreams) {
    await load();
    const t0 = performance.now();
    const r = z.analyze(new Uint8Array(bytes), JSON.stringify(settings));
    try {
      const { sport, summary } = JSON.parse(r.summary_json());
      const streams: Streams | null = withStreams
        ? {
            elapsed: r.take_elapsed(),
            power: r.take_power(),
            heartRate: r.take_heart_rate(),
            cadence: r.take_cadence(),
            altitude: r.take_altitude(),
            speed: r.take_speed(),
            wPrimeBalance: r.take_w_prime_balance(),
          }
        : null;
      const activity: AnalyzedActivity = {
        sport,
        summary,
        startTime: r.start_time(),
        curveDurations: r.take_curve_durations(),
        curveWatts: r.take_curve_watts(),
        analysisMs: performance.now() - t0,
      };
      return { activity, streams };
    } finally {
      r.free();
    }
  },

  async fitness(activities, endDay, wPrimeFallback) {
    await load();
    return JSON.parse(
      z.fitness(JSON.stringify({ activities, end_day: endDay, w_prime_fallback: wPrimeFallback })),
    );
  },

  async planWorkout(workout, ftp) {
    await load();
    return JSON.parse(z.plan_workout(JSON.stringify(workout), ftp));
  },

  async exportZwo(workout) {
    await load();
    return z.export_zwo(JSON.stringify(workout));
  },

  async exportFit(workout, timeCreated) {
    await load();
    return z.export_fit_workout(JSON.stringify(workout), timeCreated);
  },

  async fuelingDay(input) {
    await load();
    return JSON.parse(z.fueling_day(JSON.stringify(input)));
  },
};

self.onmessage = async (event: MessageEvent<Request>) => {
  const { id, method, args } = event.data;
  try {
    // The union of handler signatures can't be called generically without
    // a cast; `Request<M>` guarantees args match method.
    const handler = handlers[method] as (...a: unknown[]) => Promise<unknown>;
    const result = await handler(...args);
    const response: Response = { id, ok: true, result };
    self.postMessage(response, transferables(result));
  } catch (e) {
    const response: Response = { id, ok: false, error: e instanceof Error ? e.message : String(e) };
    self.postMessage(response);
  }
};
