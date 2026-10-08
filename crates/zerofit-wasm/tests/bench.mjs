// Times the WASM analysis of a FIT file in node (median of N runs).
//
//   node crates/zerofit-wasm/tests/bench.mjs <file.fit> [runs]
//
// Expects the wasm-bindgen nodejs output in target/wasm-pkg (or WASM_PKG).
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const pkg = resolve(process.env.WASM_PKG ?? join(root, "target/wasm-pkg"));
const wasm = createRequire(import.meta.url)(join(pkg, "zerofit_wasm.js"));
const [file, runsArg] = process.argv.slice(2);
const bytes = readFileSync(file);
const runs = Number(runsArg ?? 15);
const settings = JSON.stringify({ ftp: 260, lthr: 165, max_hr: 188, resting_hr: 48 });

wasm.analyze(bytes, settings).free(); // warm-up (compiles the module's code paths)
const times = [];
for (let i = 0; i < runs; i++) {
  const t0 = performance.now();
  const r = wasm.analyze(bytes, settings);
  r.take_power();
  r.free();
  times.push(performance.now() - t0);
}
times.sort((a, b) => a - b);
const median = times[Math.floor(times.length / 2)];
console.log(`${file}: ${bytes.length} bytes, median ${median.toFixed(1)} ms over ${runs} runs (min ${times[0].toFixed(1)})`);
