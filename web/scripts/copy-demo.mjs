// Copies the anonymized fixture rides into static/demo, gzipped (1.42 MB ->
// 0.86 MB; static hosts don't reliably compress .fit files), so the "Load
// demo data" button can fetch them; the app decompresses them with the
// browser's DecompressionStream. They are the maintainer's own recordings,
// anonymized with fit-anonymize (see crates/zerofit/tests/fixtures/README.md).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = resolve(web, "../crates/zerofit/tests/fixtures");
const dest = join(web, "static/demo");
mkdirSync(dest, { recursive: true });
for (const name of ["icu_short", "icu_laps", "icu_intervals", "wahoo_elemnt"]) {
  writeFileSync(join(dest, `${name}.fit.gz`), gzipSync(readFileSync(join(fixtures, `${name}.fit`)), { level: 9 }));
}
console.log(`copied 4 demo rides to ${dest}`);
