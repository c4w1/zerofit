// Node smoke test for the WebAssembly build.
//
//   cargo build -p zerofit-wasm --profile wasm-release --target wasm32-unknown-unknown
//   wasm-bindgen --target nodejs --out-dir target/wasm-pkg \
//       target/wasm32-unknown-unknown/wasm-release/zerofit_wasm.wasm
//   node crates/zerofit-wasm/tests/smoke.mjs
//
// Optional: WASM_PKG=<dir> to point at the wasm-bindgen output elsewhere.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const pkg = resolve(process.env.WASM_PKG ?? join(root, "target/wasm-pkg"));
const require = createRequire(import.meta.url);
const wasm = require(join(pkg, "zerofit_wasm.js"));
const fixture = (name) => readFileSync(join(root, "crates/zerofit/tests/fixtures", `${name}.fit`));

assert.match(wasm.version(), /^\d+\.\d+\.\d+$/);

// Analysis: summary JSON plus typed-array streams.
const result = wasm.analyze(fixture("icu_laps"), JSON.stringify({ ftp: 323, lthr: 165 }));
const { sport, summary: s } = JSON.parse(result.summary_json());
assert.equal(sport, "cycling");
assert.ok(s.has_power);
assert.ok(Math.abs(s.normalized_power - 254) < 1, `NP ${s.normalized_power}`);
assert.ok(Math.abs(s.average_power - 184) < 1, `avg ${s.average_power}`);
assert.ok(s.tss > 70 && s.tss < 75, `TSS ${s.tss}`);
assert.equal(s.power_zone_seconds.length, 7);
const power = result.take_power();
assert.ok(power instanceof Uint16Array);
assert.equal(power.length, s.recording_time_s);
assert.ok(result.take_elapsed() instanceof Uint32Array);
assert.ok(result.take_w_prime_balance() instanceof Float32Array);
const durations = result.take_curve_durations();
const watts = result.take_curve_watts();
assert.equal(durations.length, watts.length);
assert.equal(durations[0], 1);
result.free();

// A heart-rate-only ride: no power metrics, hrTSS from the HR settings.
const short = JSON.parse(
  wasm.analyze_summary(fixture("icu_short"), JSON.stringify({ lthr: 160, max_hr: 190, resting_hr: 50 })),
);
assert.equal(short.summary.has_power, false);
assert.equal(short.summary.normalized_power, null);
assert.ok(short.summary.hr_tss > 0);

// Fitness across activities.
const fit = JSON.parse(
  wasm.fitness(
    JSON.stringify({
      end_day: 20,
      activities: [
        { day: 10, tss: 100, curve: Array.from(durations, (d, i) => [d, watts[i]]) },
        { day: 12, tss: 60, curve: [] },
      ],
    }),
  ),
);
assert.equal(fit.days.length, 11);
assert.ok(fit.days[10].ctl > 0);
assert.equal(fit.season_curve.length, durations.length);

// Workouts: planned load and both export formats.
const workout = JSON.stringify({
  name: "Smoke 2x20",
  steps: [
    { duration_s: 600, kind: "warmup", target: { type: "ramp", from: 50, to: 75 } },
    { duration_s: 1200, target: { type: "steady", pct: 90 } },
    { duration_s: 300, kind: "recovery", target: { type: "steady", pct: 50 } },
    { duration_s: 1200, target: { type: "steady", pct: 90 } },
  ],
});
const planned = JSON.parse(wasm.plan_workout(workout, 250));
assert.equal(planned.duration_s, 3300);
assert.ok(planned.tss > 50 && planned.tss < 80, `planned TSS ${planned.tss}`);
assert.match(wasm.export_zwo(workout), /<SteadyState Duration="1200" Power="0.900"\/>/);
const fitBytes = wasm.export_fit_workout(workout, 1100000000);
assert.ok(fitBytes instanceof Uint8Array);
assert.equal(String.fromCharCode(...fitBytes.slice(8, 12)), ".FIT");

// Fueling.
const day = JSON.parse(
  wasm.fueling_day(
    JSON.stringify({
      body_mass_kg: 70,
      ftp_w: 250,
      sessions: [{ start_min: 540, duration_min: 180, intensity_factor: 0.75 }],
      next_session_start_min: 1980,
    }),
  ),
);
assert.equal(day.band, "High");
assert.ok(Math.abs(day.carbs_planned_g - day.carbs_target_g) <= 2.5);
assert.ok(day.entries.every((e) => e.time_min % 15 === 0 && e.carbs_g % 5 === 0));

// Errors surface as JavaScript exceptions with a message.
assert.throws(() => wasm.analyze(new Uint8Array([1, 2, 3]), ""), /FIT decoding failed/);
assert.throws(() => wasm.analyze_summary(fixture("icu_short"), "{"), /invalid settings/);
assert.throws(() => wasm.fueling_day('{"body_mass_kg": 0, "sessions": []}'), /body mass/);

console.log(
  `wasm smoke test ok: icu_laps NP ${s.normalized_power.toFixed(1)} W, TSS ${s.tss.toFixed(1)}; ` +
    `planned TSS ${planned.tss.toFixed(1)}; fueling ${day.carbs_g_per_kg.toFixed(2)} g/kg`,
);
