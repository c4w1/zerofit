// Builds crates/zerofit-wasm for the browser and writes the wasm-bindgen
// output to src/lib/wasm/pkg:
//
//   1. cargo build --profile wasm-release --target wasm32-unknown-unknown
//   2. wasm-bindgen --target web (needs wasm-bindgen-cli 0.2.129)
//   3. wasm-opt -Oz (binaryen, from devDependencies)
//
// Prints the module size before and after wasm-opt, raw and gzipped.
import { execFileSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(web, "..");
const out = join(web, "src/lib/wasm/pkg");
const run = (cmd, args, opts = {}) => execFileSync(cmd, args, { stdio: "inherit", cwd: root, ...opts });

run("cargo", ["build", "-p", "zerofit-wasm", "--profile", "wasm-release", "--target", "wasm32-unknown-unknown"]);
run("wasm-bindgen", [
  "--target", "web",
  "--out-dir", out,
  join(root, "target/wasm32-unknown-unknown/wasm-release/zerofit_wasm.wasm"),
]);

const wasm = join(out, "zerofit_wasm_bg.wasm");
const size = (f) => {
  const bytes = readFileSync(f);
  return `${(statSync(f).size / 1024).toFixed(1)} KiB raw, ${(gzipSync(bytes, { level: 9 }).length / 1024).toFixed(1)} KiB gzip`;
};
const before = size(wasm);
const wasmOpt = join(web, "node_modules/.bin", process.platform === "win32" ? "wasm-opt.cmd" : "wasm-opt");
run(
  wasmOpt,
  [
    "-Oz",
    "--enable-bulk-memory",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    wasm,
    "-o",
    wasm,
  ],
  { shell: process.platform === "win32" },
);
console.log(`zerofit_wasm_bg.wasm: ${before} -> ${size(wasm)} (wasm-opt -Oz)`);
