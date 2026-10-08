// Node smoke test for the WebAssembly build.
//
//   cargo build -p zerofit-analytics-wasm --release --target wasm32-unknown-unknown
//   wasm-bindgen --target nodejs --out-dir target/wasm-pkg \
//       target/wasm32-unknown-unknown/release/zerofit_analytics_wasm.wasm
//   node crates/zerofit-analytics-wasm/tests/smoke.mjs
//
// Optional: WASM_PKG=<dir> to point at the wasm-bindgen output elsewhere.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const pkg = process.env.WASM_PKG ?? join(root, "target/wasm-pkg");
const require = createRequire(import.meta.url);
const wasm = require(join(pkg, "zerofit_analytics_wasm.js"));
const fixture = (name) => readFileSync(join(root, "crates/zerofit/tests/fixtures", `${name}.fit`));

assert.match(wasm.version(), /^\d+\.\d+\.\d+$/);

// A ride with power: NP, IF and TSS against the FTP intervals.icu used.
const laps = JSON.parse(wasm.analyze(fixture("icu_laps"), JSON.stringify({ ftp: 323 })));
assert.equal(laps.sport, "cycling");
const s = laps.summary;
assert.ok(s.has_power);
assert.ok(Math.abs(s.normalized_power - 254) < 1, `NP ${s.normalized_power}`);
assert.ok(Math.abs(s.average_power - 184) < 1, `avg ${s.average_power}`);
assert.ok(s.tss > 70 && s.tss < 75, `TSS ${s.tss}`);
assert.equal(s.power_zone_seconds.length, 7);
assert.ok(s.power_curve.length > 10);
assert.equal(s.resample_report.records, 4229);

// A heart-rate-only ride: no power metrics, hrTSS from the HR settings.
const short = JSON.parse(
  wasm.analyze(fixture("icu_short"), JSON.stringify({ lthr: 160, max_hr: 190, resting_hr: 50 })),
);
assert.equal(short.summary.has_power, false);
assert.equal(short.summary.normalized_power, null);
assert.ok(short.summary.hr_tss > 0);

// Errors surface as JavaScript exceptions with a message.
assert.throws(() => wasm.analyze(new Uint8Array([1, 2, 3]), ""), /FIT decoding failed/);
assert.throws(() => wasm.analyze(fixture("icu_short"), "{"), /invalid settings/);

console.log(
  `wasm smoke test ok: icu_laps NP ${s.normalized_power.toFixed(1)} W, TSS ${s.tss.toFixed(1)}; ` +
    `icu_short hrTSS ${short.summary.hr_tss.toFixed(1)}`,
);
