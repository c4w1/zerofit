// Automated accessibility checks (axe-core, WCAG 2.1 A and AA rules) on
// every page, in light and dark mode. axe catches contrast, labels, roles
// and landmark problems; keyboard use of the charts is covered in
// flows.spec.ts.
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const pages = ["/", "/upload/", "/activities/", "/fitness/", "/plan/", "/fueling/", "/settings/"];

for (const scheme of ["light", "dark"] as const) {
  test(`no axe violations (${scheme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto("/");
    await expect(page.getByText("Across 4 activities")).toBeVisible({ timeout: 30_000 });
    const all: string[] = [];
    const visit = async (path: string) => {
      await page.goto(path);
      await page.waitForLoadState("networkidle");
      await page.waitForTimeout(800);
      const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
      for (const v of results.violations) {
        all.push(`${path} [${v.id}] ${v.help}: ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(" | ")}`);
      }
    };
    for (const p of pages) await visit(p);
    // The activity detail page, with charts.
    await page.goto("/activities/");
    await page.getByRole("link", { name: /Group ride/ }).click();
    await expect(page.getByTestId("analysis-ms")).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"]).analyze();
    for (const v of results.violations) all.push(`/activity/ [${v.id}] ${v.help}: ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(" | ")}`);
    expect(all, all.join("\n")).toEqual([]);
  });
}
