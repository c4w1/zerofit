// Every page is a static shell that runs entirely in the browser: no
// server rendering (the data lives in this browser's IndexedDB), and every
// route is prerendered so GitHub Pages can serve it as a file.
export const prerender = true;
export const ssr = false;
export const trailingSlash = "always";
