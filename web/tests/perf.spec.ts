// In-browser timing: analyze the synthetic 4-hour ride
// (`cargo run -p zerofit-bench --example synthetic-fit -- target/ride-4h.fit 14400`)
// five times and record the worker's decode+analysis time and the
// end-to-end time from file selection to the result on screen.
// Writes test-results/perf.json. Skipped if the file hasn't been generated.
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const ride = fileURLToPath(new URL("../../target/ride-4h.fit", import.meta.url));

test("analyze a 4-hour ride in the browser", async ({ page, browserName }) => {
  test.skip(!existsSync(ride), "target/ride-4h.fit not generated");
  await page.goto("/");
  await expect(page.getByText("Across 4 activities")).toBeVisible({ timeout: 30_000 });
  await page.getByRole("link", { name: "Upload", exact: true }).click();

  const worker: number[] = [];
  const endToEnd: number[] = [];
  for (let i = 0; i < 5; i++) {
    // A fresh page each time, so no earlier result is on screen.
    await page.goto("/upload/");
    await expect(page.getByTestId("file-input")).toBeEnabled();
    const t0 = Date.now();
    await page.getByTestId("file-input").setInputFiles(ride);
    const line = page.getByText(/ride-4h\.fit.*Analyzed in (\d+) ms/);
    await expect(line).toBeVisible();
    endToEnd.push(Date.now() - t0);
    worker.push(Number((await line.textContent())?.match(/Analyzed in (\d+) ms/)?.[1]));
  }
  const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
  const result = {
    browser: browserName,
    file: "synthetic 4 h ride (13,888 records, 236 KB)",
    worker_ms: worker,
    worker_median_ms: median(worker),
    end_to_end_ms: endToEnd,
    end_to_end_median_ms: median(endToEnd),
  };
  mkdirSync("test-results", { recursive: true });
  writeFileSync("test-results/perf.json", JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
  expect(result.worker_median_ms).toBeLessThan(2000);
});
