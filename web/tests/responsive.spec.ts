// Phone width (Pixel 7 project): no page scrolls sideways, and the main
// navigation is reachable.
import { expect, test } from "@playwright/test";

const pages = ["/", "/upload/", "/activities/", "/fitness/", "/plan/", "/fueling/", "/settings/"];

test("no horizontal overflow at phone width", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Across 4 activities")).toBeVisible({ timeout: 30_000 });
  const overflowing: string[] = [];
  for (const p of pages) {
    await page.goto(p);
    await page.waitForTimeout(1200);
    const { scroll, client } = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      client: document.documentElement.clientWidth,
    }));
    if (scroll > client + 1) overflowing.push(`${p}: ${scroll} > ${client}`);
  }
  await page.goto("/activities/");
  await page.getByRole("link", { name: /Group ride/ }).click();
  await expect(page.getByTestId("analysis-ms")).toBeVisible();
  await page.waitForTimeout(800);
  const w = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
  if ((w[0] ?? 0) > (w[1] ?? 0) + 1) overflowing.push(`/activity/: ${w[0]} > ${w[1]}`);
  expect(overflowing).toEqual([]);
  await expect(page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Settings" })).toBeAttached();
});
