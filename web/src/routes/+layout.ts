// Every route is prerendered to static HTML at build time, so headings and
// text paint before any JavaScript runs; GitHub Pages serves the files.
// All data lives in this browser's IndexedDB, so everything that depends on
// it renders in the browser after hydration (onMount / $effect).
export const prerender = true;
export const trailingSlash = "always";
