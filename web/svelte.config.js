import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Fully static: every route is prerendered as an app shell, and all
    // work happens in the browser. GitHub Pages serves 404.html for
    // unknown paths.
    adapter: adapter({ fallback: "404.html", strict: true }),
    // "/zerofit" on GitHub Pages; empty for local dev and tests.
    paths: { base: process.env.BASE_PATH ?? "" },
  },
};

export default config;
