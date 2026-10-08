# zerofit web app

A fully client-side training app built on the zerofit crates. It covers FIT
upload, activity analysis, fitness trends, workout planning and fueling
plans. The Rust decoder, analytics and fueling crates are compiled to one
WebAssembly module, which runs in a Web Worker.

**Your data never leaves your browser.** There is no server, no account,
and no analytics or tracking. Files are analyzed on your device and stored
in this browser's IndexedDB. The only network requests are for the app's
own static files (including the demo rides).

## Develop

You need Rust, the `wasm32-unknown-unknown` target, `wasm-bindgen-cli`
0.2.129 (matching the crate) and Node 22+.

```sh
npm ci
npm run prepare-assets   # build the WASM (LTO + wasm-opt -Oz) and copy the gzipped demo rides
npm run dev              # http://localhost:5173
npm run check            # svelte-check (TypeScript, a11y warnings fail the build)
npm run build            # static site in build/ (BASE_PATH=/zerofit for GitHub Pages)
npm run test:e2e         # Playwright against `vite preview`
```

## Architecture

```
UI thread (Svelte 5)                         Web Worker
─────────────────────                        ──────────────────────────────
routes/*  ─ app state (runes) ─ client.ts ──► worker.ts ─► zerofit_wasm.wasm
              │                  call(method, args, transfer)    (decoder + analytics + fueling)
              ▼                  ◄── result + transferred typed arrays
          IndexedDB (db.ts): FIT bytes, summaries, settings, plan
```

- `src/lib/worker`: a typed RPC over `postMessage`. The worker owns the
  WASM module. Per-second streams come back as typed arrays whose buffers
  are transferred, not copied.
- `src/lib/db.ts`: the raw IndexedDB API. Activities are keyed by a SHA-256
  of the file, so re-uploading a file replaces it.
- `src/lib/state.svelte.ts`: the app state as Svelte 5 runes. A first visit
  loads the demo: the four anonymized fixture rides, re-dated into the last
  ten days (disclosed in the UI), plus a sample week.
- Charts: uPlot for time series (synced cursor and zoom, keyboard control);
  ECharts (modular, lazy-loaded) for zones, power curves and the fueling
  timeline. Every chart has a data-table alternative.
- Pages are prerendered to static HTML. Everything data-dependent renders
  after hydration into placeholders that already have their final size.

## Tests (Playwright)

- `flows.spec.ts`:
  - load the demo and open an activity (charts, keyboard, data table);
  - build a workout and check planned TSS, the `.zwo` and FIT exports, and
    that it persists;
  - view the fueling plan and its disclaimer;
  - upload fixture files, plus an invalid file.
- `a11y.spec.ts`: axe-core with WCAG 2.1 A/AA rules on every page, in light
  and dark mode. No violations.
- `responsive.spec.ts`: no horizontal overflow at Pixel 7 width.
- `perf.spec.ts`: analyzes the synthetic 4 h ride five times. Writes
  `test-results/perf.json`.

## Measurements (2026-10-08)

Measured on a Core Ultra 7 165U laptop running Windows. The machine was
not idle.

**WASM module**: 255 KB raw, 110 KB gzip after LTO + `wasm-opt -Oz`, down
from 364 KB / 123 KB with release defaults. See
`crates/zerofit-wasm/README.md` for the full table.

**Analyzing a 4-hour ride in the browser** (Chromium 156, Playwright,
synthetic ride, 13,888 records, 236 KB): median of 5 runs.

| | Median |
|---|---|
| Worker: decode + resample + every metric + full power curve + W'bal | **70 ms** |
| End to end: file selected → result on screen (adds file read, SHA-256, IndexedDB write, UI) | **122 ms** |

**JavaScript**: ECharts is the largest chunk (559 KB raw / 191 KB gzip). It
is loaded only by pages that draw ECharts charts, after their text has
rendered. uPlot is 56 KB / 25 KB.

**Lighthouse 13.5.0** (simulated mobile and slow 4G unless noted). Each run
is a first visit, so the page also downloads the WASM module and the demo
rides (≈ 1.2–1.4 MB in total):

| Page | Perf before | Perf after | A11y | Best practices | SEO | LCP after | CLS after |
|---|---|---|---|---|---|---|---|
| Overview | 65 | **100** | 100 | 100 | 100 | 1.5 s | 0.031 |
| Overview (desktop) | 91 | **100** | 100 | 100 | 100 | 0.4 s | 0.003 |
| Plan | — | **98** | 100 | 100 | 100 | 2.3 s | 0.038 |
| Activity | 58 | 72 | 100 | 100 | 100 | 7.6 s | 0.031 |
| Fueling | 46 | 77 | 100 | 100 | 100 | 6.4 s | 0.031 |
| Fitness | 37 | 74 | 100 | 100 | 100 | 7.6 s | 0.031 |

"Before" is the first build: client-rendered shells (`ssr = false`), with
ECharts and uPlot imported eagerly. "After" adds three changes:

- pages prerendered to static HTML;
- chart libraries loaded on mount;
- placeholders sized to the final layout.

These moved CLS from 0.16–0.44 to ≤ 0.04 and TBT from 30–620 ms to
0–190 ms. The deep pages (Activity, Fitness, Fueling) are still limited by
the first visit's data: their largest element is analysis output, which
needs the demo rides and the WASM module first. On simulated slow 4G that
download dominates. Returning visits read IndexedDB and skip it. All scores
are single runs; expect a few points of variance.
