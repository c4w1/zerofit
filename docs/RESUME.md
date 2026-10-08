# Resume bullets: zerofit

Every number below was measured in this repository. The table after the
bullets says where each one comes from, so it can be re-checked before you
use it. Items in [brackets] don't exist yet: fill them in or drop them.

---

**zerofit: privacy-first cycling analytics in Rust and WebAssembly**
([live demo](https://c4w1.github.io/zerofit/) · [source](https://github.com/c4w1/zerofit))

- Built a zero-copy, `no_std`, panic-free Rust decoder for Garmin FIT files.
  It decodes and scales every field **20–28× faster than the `fitparser`
  crate with zero heap allocations** (vs. up to 1.58 M), and was hardened
  by **179 M fuzzing executions** with no library crash, 96 generated
  corruption cases and Garmin's FitCSVTool as ground truth.
- Implemented a training-analytics library (normalized power, TSS, CP/W′,
  W′ balance, CTL/ATL) whose output **matches intervals.icu to the joule on
  total work, and within 0.8 % on NP and 1.5 % on TSS**, investigating
  every difference before tuning. Its vectorized exact power curve covers
  every duration of a 6-hour ride (233 M windows) in **75 ms**, and
  property testing caught two "obvious" invariants that are false.
- Shipped a fully client-side SvelteKit app. Rust runs as a single
  **110 KB gzipped WebAssembly module** (−30 % raw via LTO + `wasm-opt`) in
  a Web Worker, and analyzes a 4-hour ride in **70 ms in the browser**. It
  has no server, accounts or tracking, and stores data in IndexedDB.
  Lighthouse scores **100 on the landing page** (performance,
  accessibility, best practices, SEO), with **0 axe-core WCAG 2.1 AA
  violations** across all pages in light and dark mode.
- Added a rules-based sports-nutrition engine that turns planned training
  into carbohydrate and protein plans from ACSM/IOC consensus, with every
  rule unit-tested at its boundaries and property-tested for monotonicity.
  The project has **349 Rust tests and 9 Playwright end-to-end and
  accessibility tests**, and CI deploys to GitHub Pages
  [crates published on crates.io: ___].

---

Shorter variant (one line per project area):

- Rust FIT decoder: zero-copy, `no_std`, panic-free; 20–28× faster than `fitparser`, 0 allocations, 179 M fuzz executions.
- Analytics validated against intervals.icu (work exact, NP ≤ 0.8 %, TSS ≤ 1.5 %); 6 h exact power curve in 75 ms.
- Client-side WASM app: 110 KB gzipped module, 4 h ride analyzed in 70 ms in-browser, Lighthouse 100 landing page, 0 axe violations.

## Where each number comes from

| Claim | Source |
|---|---|
| 20–28× faster than `fitparser`, 0 vs up to 1,581,064 allocations | `README.md` § Performance (criterion, `cargo bench -p zerofit-bench`; `alloc_count` bench) |
| 179 M fuzz executions, no crash in the library | `README.md` § How correctness is verified, `fuzz/README.md` |
| 96 generated corruption cases; FitCSVTool ground truth | `crates/zerofit/tests/error_fixtures.rs`; `cargo xtask expected` |
| Work exact / 0.007 %; NP ≤ 0.8 %; TSS ≤ 1.5 % vs intervals.icu | `crates/zerofit-analytics/README.md` § Validation (session values in intervals.icu's FIT export; the hand-recorded file `intervals_icu.json` is still a template) |
| 6 h exact power curve in 75 ms (233 M windows) | `crates/zerofit-analytics/README.md` § Performance (`cargo bench -p zerofit-bench --bench analytics`) |
| Two false invariants found by property tests (MMP monotonicity, NP ≥ average) | `crates/zerofit-analytics/tests/proptest.rs`, `mmp_can_increase`, `np_below_average_at_edges` |
| 110 KB gzip / 255 KB raw WASM; −30 % raw vs release defaults | `crates/zerofit-wasm/README.md` § Size and speed |
| 4 h ride analyzed in 70 ms in the browser (median of 5, Chromium) | `web/README.md` § Measurements (`web/tests/perf.spec.ts`) |
| Lighthouse 100 on the landing page; accessibility 100 on all measured pages | `web/README.md` § Measurements (Lighthouse 13.5.0) |
| 0 axe-core WCAG 2.1 A/AA violations, all pages, light and dark | `web/tests/a11y.spec.ts` |
| 349 Rust tests; 9 Playwright tests | `cargo test --workspace --all-features` (349 passed, 2026-10-08); `npm run test:e2e` (9 passed) |

Numbers that would strengthen the bullets but don't exist yet:

- [crates.io downloads: the crates aren't published]
- [visitors or users: there's no analytics, by design]
- [intervals.icu error on hand-recorded values: fill in `crates/zerofit-analytics/tests/fixtures/intervals_icu.json`]
