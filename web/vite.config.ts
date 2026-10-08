import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [sveltekit()],
  // The analysis worker is an ES module worker that imports the
  // wasm-bindgen output.
  worker: { format: "es" },
  build: { target: "es2022" },
  server: { fs: { allow: [".."] } },
});
