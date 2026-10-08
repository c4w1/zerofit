// The flows a visitor goes through: demo, activity, workout builder,
// fueling plan, upload.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";

const fixture = (name: string) => fileURLToPath(new URL(`../../crates/zerofit/tests/fixtures/${name}`, import.meta.url));

/** Fails the test on any uncaught error or console error. */
function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  return errors;
}

async function demoLoaded(page: Page) {
  await page.goto("/");
  await expect(page.getByText("Across 4 activities")).toBeVisible({ timeout: 30_000 });
}

test("load demo and open an activity", async ({ page }) => {
  const errors = watchErrors(page);
  await demoLoaded(page);
  await page.getByRole("link", { name: "Activities", exact: true }).click();
  const rows = page.locator("tbody tr");
  await expect(rows).toHaveCount(4);

  // Sorting by TSS: the column reads in descending order.
  await page.getByRole("button", { name: /^TSS/ }).click();
  await expect(page.getByRole("columnheader", { name: /TSS/ })).toHaveAttribute("aria-sort", "descending");
  const tss = (await rows.locator("td:nth-child(5)").allTextContents()).map((t) => Number(t.replace(/[^\d.]/g, "")));
  expect(tss).toEqual([...tss].sort((a, b) => b - a));

  await page.getByRole("link", { name: "Group ride with laps (demo)" }).click();
  await expect(page.getByRole("heading", { name: "Group ride with laps (demo)" })).toBeVisible();
  // NP of icu_laps: 254 W (validated against intervals.icu in zerofit-analytics).
  await expect(page.getByTestId("np")).toHaveText(/254\s*W/);
  await expect(page.getByTestId("analysis-ms")).toContainText("ms on this device");
  // Four synced charts, keyboard focusable.
  const charts = page.getByRole("application");
  await expect(charts).toHaveCount(4);
  await charts.first().focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("+");
  await expect(page.getByRole("heading", { name: "Power curve (mean-maximal power)" })).toBeVisible();
  await page.getByText("Show data as a table").first().click();
  await expect(page.getByRole("cell", { name: "Z4 Threshold" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("build a workout and export it", async ({ page }) => {
  const errors = watchErrors(page);
  await demoLoaded(page);
  await page.getByRole("link", { name: "Plan", exact: true }).click();
  await expect(page.getByTestId("week-tss")).not.toHaveText("—");

  await page.getByRole("button", { name: /Add workout on Monday/ }).click();
  await page.getByTestId("workout-name").fill("E2E threshold");
  const tss = page.getByTestId("planned-tss");
  await expect(tss).not.toHaveText("—");
  const before = Number(await tss.textContent());

  // 4 x 4 min at 115 % adds load: planned TSS goes up live.
  await page.getByTestId("add-set").click();
  await expect.poll(async () => Number(await tss.textContent())).toBeGreaterThan(before);

  const zwo = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download .zwo (Zwift)" }).click();
  const zwoFile = await zwo;
  expect(zwoFile.suggestedFilename()).toBe("E2E_threshold.zwo");
  const xml = readFileSync(await zwoFile.path(), "utf8");
  expect(xml).toContain("<name>E2E threshold</name>");
  expect(xml.match(/<SteadyState Duration="240" Power="1.150"\/>/g)).toHaveLength(4);

  const fit = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download .fit workout" }).click();
  const fitBytes = readFileSync(await (await fit).path());
  expect(fitBytes.subarray(8, 12).toString("latin1")).toBe(".FIT");

  // The plan persists across a reload (IndexedDB).
  await page.reload();
  await expect(page.getByRole("button", { name: /E2E threshold/ })).toBeVisible();
  expect(errors).toEqual([]);
});

test("view the fueling plan", async ({ page }) => {
  const errors = watchErrors(page);
  await demoLoaded(page);
  await page.getByRole("link", { name: "Fueling", exact: true }).click();
  await expect(page.getByRole("note", { name: "Disclaimer" })).toContainText("not medical or dietary advice");

  // Saturday has the sample week's 4-hour ride: a high or very high day
  // with in-ride feeds and rapid recovery (Sunday's ride is < 24 h away).
  await page.getByRole("tab", { name: /Sat/ }).click();
  const gkg = Number((await page.getByTestId("carbs-gkg").textContent())?.replace(/[^\d.]/g, ""));
  expect(gkg).toBeGreaterThanOrEqual(6);
  const table = page.getByTestId("fuel-table");
  await expect(table.getByRole("row", { name: /On the bike/ }).first()).toBeVisible();
  await expect(table.getByRole("row", { name: /Recovery, hour 1/ })).toBeVisible();
  await expect(page.getByText(/glucose \+ fructose/)).toBeVisible();
  expect(errors).toEqual([]);
});

test("upload a fixture file", async ({ page }) => {
  const errors = watchErrors(page);
  await demoLoaded(page);
  await page.getByRole("link", { name: "Upload", exact: true }).click();
  await page.getByTestId("file-input").setInputFiles([fixture("icu_laps.fit"), fixture("wahoo_elemnt.fit")]);
  const results = page.getByRole("heading", { name: "Results" }).locator("..");
  await expect(results.getByText(/icu_laps\.fit.*Analyzed in \d+ ms/)).toBeVisible();
  await expect(results.getByText(/wahoo_elemnt\.fit.*Analyzed in \d+ ms/)).toBeVisible();
  // Re-uploading a demo file replaces it (same content id); the Wahoo
  // ride does too: still four activities, now under the uploaded names.
  await page.getByRole("link", { name: "Activities", exact: true }).click();
  await expect(page.locator("tbody tr")).toHaveCount(4);
  await expect(page.getByRole("link", { name: "icu_laps" })).toBeVisible();

  // A file that isn't FIT fails with a message, without breaking the page.
  await page.getByRole("link", { name: "Upload", exact: true }).click();
  await page.getByTestId("file-input").setInputFiles({ name: "notes.fit", mimeType: "application/octet-stream", buffer: Buffer.from("not a fit file") });
  await expect(page.getByText(/notes\.fit.*could not be read: FIT decoding failed/)).toBeVisible();
  expect(errors).toEqual([]);
});
