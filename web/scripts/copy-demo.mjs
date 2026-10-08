// Copies the anonymized fixture rides into static/demo so the "Load demo
// data" button can fetch them. They are the maintainer's own recordings,
// anonymized with fit-anonymize (see crates/zerofit/tests/fixtures/README.md).
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const fixtures = resolve(web, "../crates/zerofit/tests/fixtures");
const dest = join(web, "static/demo");
mkdirSync(dest, { recursive: true });
for (const name of ["icu_short", "icu_laps", "icu_intervals", "wahoo_elemnt"]) {
  copyFileSync(join(fixtures, `${name}.fit`), join(dest, `${name}.fit`));
}
console.log(`copied 4 demo rides to ${dest}`);
