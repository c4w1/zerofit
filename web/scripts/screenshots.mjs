// Takes the README screenshots from the production build:
//   npm run build && node scripts/screenshots.mjs
// Writes docs/images/{hero,hero-dark,fueling,plan}.png at 1280x900.
import { chromium } from "@playwright/test";
import { execSync, spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const web = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = resolve(web, "../docs/images");
mkdirSync(out, { recursive: true });
const port = 4177;
const server = spawn(process.execPath, [join(web, "node_modules/vite/bin/vite.js"), "preview", "--port", String(port), "--strictPort"], { cwd: web });
const stop = () => {
  try {
    if (process.platform === "win32") execSync(`taskkill /pid ${server.pid} /T /F`, { stdio: "ignore" });
    else server.kill();
  } catch {}
};
process.on("exit", stop);
await new Promise((r) => setTimeout(r, 3000));

const browser = await chromium.launch();
async function shoot(scheme, shots) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 }, deviceScaleFactor: 1.5, colorScheme: scheme });
  const page = await ctx.newPage();
  const url = (p) => `http://localhost:${port}${p}`;
  await page.goto(url("/"));
  await page.getByText("Across 4 activities").waitFor({ timeout: 30000 });
  for (const [name, go] of shots) {
    await go(page, url);
    await page.waitForTimeout(1500);
    await page.screenshot({ path: join(out, `${name}.png`) });
  }
  await ctx.close();
}
const activity = async (page, url) => {
  await page.goto(url("/activities/"));
  await page.getByRole("link", { name: /Group ride/ }).click();
  await page.getByTestId("analysis-ms").waitFor();
};
await shoot("light", [
  ["hero", activity],
  ["fueling", async (page, url) => {
    await page.goto(url("/fueling/"));
    await page.getByRole("tab", { name: /Sat/ }).click();
  }],
  ["plan", async (page, url) => {
    await page.goto(url("/plan/"));
    await page.getByRole("button", { name: /Long ride with tempo/ }).click();
    await page.evaluate(() => window.scrollTo(0, 0));
  }],
]);
await shoot("dark", [["hero-dark", activity]]);
await browser.close();
stop();
console.log(`screenshots written to ${out}`);
process.exit(0);
