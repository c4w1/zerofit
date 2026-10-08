// Theme tracking for charts: canvas charts can't use CSS variables directly,
// so they read the computed colours and re-render when the theme changes.

export const theme = $state({ version: 0, dark: false });

let started = false;

export function startThemeTracking(): void {
  if (started || typeof window === "undefined") return;
  started = true;
  const update = () => {
    const forced = document.documentElement.dataset.theme;
    theme.dark = forced ? forced === "dark" : window.matchMedia("(prefers-color-scheme: dark)").matches;
    theme.version++;
  };
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", update);
  new MutationObserver(update).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
  update();
}

export function currentThemeChoice(): "light" | "dark" | "system" {
  const t = document.documentElement.dataset.theme;
  return t === "light" || t === "dark" ? t : "system";
}

export function setTheme(mode: "light" | "dark" | "system"): void {
  if (mode === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = mode;
  try {
    if (mode === "system") localStorage.removeItem("zerofit-theme");
    else localStorage.setItem("zerofit-theme", mode);
  } catch {
    // Storage blocked (private mode): the choice lasts for this page only.
  }
}

/** A CSS custom property's current value, e.g. `cssVar("--power")`. */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}
