import { expect, test } from "@playwright/test";

// First visit: the demo loads by itself and the overview fills in.
test("first visit loads the demo automatically", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Today's form" })).toBeVisible();
  await expect(page.getByText("Across 4 activities")).toBeVisible({ timeout: 30_000 });
  await expect(page.getByRole("heading", { name: "Estimated FTP" })).toBeVisible();
  expect(errors).toEqual([]);
});
