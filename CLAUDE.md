# CLAUDE.md

Guidance for working on **zerofit**, a zero-copy, `no_std`, panic-free Rust
decoder for FIT activity files. This is a portfolio project aimed at
systems/embedded roles: correctness, performance, tests and docs matter more
than feature count.

## Status and scope

- **Done:** raw protocol layer (slice `Decoder`, streaming `ReadDecoder`,
  minimal `Encoder`), generated typed profile layer for `file_id`, `record`,
  `lap`, `session`, `event`, `device_info`, `activity`, `hrv`, developer
  field resolution, FitCSVTool ground truth for fixtures, generated error
  fixtures, three fuzz targets, criterion benchmarks vs `fitparser`,
  `fit-dump` and `fit-anonymize` examples, publish-ready metadata.
- **Not published.** The maintainer publishes after reviewing the FIT SDK
  license. Never run `cargo publish` without `--dry-run`.
- **Also done:** `zerofit-analytics` (training metrics on 1 Hz streams,
  workouts), `zerofit-fueling` (rules-based carbohydrate/protein
  periodization), `zerofit-wasm` (all crates in one WASM module) and the
  client-side web app in `web/` (SvelteKit, static, deployed to GitHub
  Pages from `main` by CI). Docs: `docs/INTERVIEW.md`, `docs/RESUME.md`.
- **Out of scope:** any server, accounts, tracking or analytics in the app.
  The app must keep working with no network after load, apart from its own
  static files.

## Crate map

```
crates/zerofit/          raw protocol decoder + encoder, #![no_std], features: alloc, std
crates/zerofit-profile/  typed profile layer; src/generated/ is codegen output (checked in)
crates/zerofit-analytics/ training metrics (NP, TSS, MMP, CP, W'bal, CTL...), #![no_std] + alloc,
                         features: fit (default; zerofit + zerofit-profile), serde
crates/zerofit-fueling/  fueling day plans from planned sessions, #![no_std] + alloc, no deps, feature: serde
crates/zerofit-wasm/     publish = false; all crates in one wasm-bindgen module for the web app
web/                     SvelteKit static app (TypeScript); WASM runs in a Web Worker; IndexedDB storage
crates/zerofit-codegen/  publish = false; Profile.xlsx -> profile-subset.json -> Rust
crates/zerofit-bench/    publish = false; criterion + allocation-count benches vs fitparser
xtask/                   publish = false; `cargo xtask codegen|extract|expected`
fuzz/                    cargo-fuzz crate, own workspace, nightly only (run in WSL/Linux)
docs/INTERVIEW.md        design walkthrough and Q&A
```

## Architecture

- **Two layers.** The raw layer (`zerofit`) knows only the protocol: file
  header, CRC-16, record headers, definition messages, data messages, base
  types and invalid sentinels. It never names a message or field, with one
  exception: field 253 (`timestamp`) is protocol-level because compressed
  timestamp headers depend on it. The profile layer maps global message and
  field numbers to names, scale/offset, units and enums. (`fit-anonymize` is
  a tool and names a few profile numbers, documented in the file.)
- **Zero-copy definitions.** A definition message's field list is kept as a
  borrowed `&'a [u8]` of 3-byte triples, so `Definition<'a>` is `Copy`. The
  decoder holds a fixed table of 16 local-message slots (no heap).
  `DataMessage<'a>` carries a copy of its definition, so the slice decoder is
  a plain `Iterator` whose items borrow only the input.
- **Lazy decoding.** Iterating messages costs only record-header parsing and a
  slice split. Field values are decoded (with per-definition endianness) on
  demand. Message size and the offset of field 253 are precomputed when a
  definition is parsed.
- **One record parser.** `decoder::parse_record` parses one record from a
  byte slice given the decoder state and returns the new state. The slice
  decoder and `ReadDecoder` both call it, so their errors and offsets are
  identical; on short input it returns `UnexpectedEof { needed }`, which the
  streaming decoder treats as "read more".
- **Errors.** `Error { offset: u64, kind: ErrorKind }` is `Copy + PartialEq`.
  The offset is the absolute byte position in the input stream. After the
  first error the decoders are fused. I/O errors live in the std-only
  `ReadError`.
- **CRC timing.** The slice decoder checks a complete file's CRC before
  yielding its header; the streaming decoder checks it at the file end.
- **Chained files.** After a file's CRC, if bytes remain, a new header is
  parsed and the definition table is reset.

## no_std tradeoffs

| Constraint | Consequence |
|---|---|
| No `std::io` in core | `ReadDecoder` behind `std`; no_std users feed slices |
| No `alloc` | Only the slice API; definitions borrow the input; fixed 16-slot table |
| Errors can't hold `io::Error`/`String` | Core error is plain `Copy` data |
| No `HashMap` | Fixed arrays; developer-field resolution needs `alloc` (BTreeMap) |
| No float math like `round` in `core` | Profile scaling uses only `/` and `-` |

## Invariants (non-negotiable)

- **Never panic on malformed input.** Library code must not use `unwrap`,
  `expect`, `panic!`, slice indexing (`a[i]`, `a[i..j]`) or unchecked
  arithmetic. These are denied by workspace lints. Use `get`, `split_at_checked`,
  `checked_*`/`wrapping_*`, and return `Error`s with byte offsets.
- **No `unsafe`** (`unsafe_code = "forbid"`) in every crate except
  `zerofit-bench`, whose counting `GlobalAlloc` needs it, and
  `zerofit-wasm`, whose `#[wasm_bindgen]` exports expand to FFI
  glue (`deny` + documented `#[allow]`s).
- **Every public item has rustdoc** (`missing_docs = "deny"`), with a runnable
  example for types and functions users call directly.
- **The cores must build for `thumbv7em-none-eabihf`**: `zerofit` with
  `--no-default-features`, `zerofit-profile` without features,
  `zerofit-analytics` with `--no-default-features` (needs only `alloc`),
  `zerofit-fueling` without features.
  Never use `std::` in core code; use `core::` or `alloc::` behind the feature.
- **Expected fixture values never come from zerofit.** They come from
  FitCSVTool via `cargo xtask expected`; `xtask` must not depend on
  `zerofit`/`zerofit-profile` (a test enforces it).
- **Generated code is never edited by hand.** Change `zerofit-codegen` and
  run `cargo xtask codegen`; CI runs `cargo xtask codegen --check`.
- Test code may use `unwrap`/indexing: unit tests are exempted in
  `clippy.toml`; integration test crates start with a crate-level
  `#![allow(...)]`. Examples are tools and may allow arithmetic lints.

## Conventions

- **Small commits, one logical change each.** Run the full check suite (below)
  before every commit, then report what changed and what's next.
- **Ask before adding any dependency.** Approved: `thiserror` (no_std,
  `default-features = false`), `libm` (zerofit-analytics), `serde` (optional,
  no_std, zerofit-analytics, zerofit-fueling), `wasm-bindgen` + `serde`/`serde_json`
  (zerofit-wasm), the web app's npm packages in `web/package.json`
  (SvelteKit, Svelte, Vite, TypeScript, uPlot, ECharts, Playwright,
  @axe-core/playwright, binaryen), `proptest` (dev), `serde` + `serde_json` (dev,
  fixtures; normal deps of xtask/codegen; dev-dep for examples), `calamine`
  (codegen only), `criterion` + `fitparser` (zerofit-bench), `libfuzzer-sys`
  (fuzz crate). Anything else needs approval.
- **Never commit Garmin SDK files:** no `Profile.xlsx`, no SDK sample `.fit`
  files, no FIT SDK source. Fixtures must be the user's own recordings, run
  through `fit-anonymize` and privacy-checked before committing.
- MSRV is **1.85** (edition 2024) for `zerofit`, `zerofit-profile`,
  `zerofit-analytics` and `zerofit-fueling`. Don't use newer std APIs there without checking. Tools may use stable.
- Tests: unit tests next to the code (`#[cfg(test)] mod tests`); integration
  tests in `crates/*/tests/`. Synthetic FIT files are built with the dev-only
  writer in `crates/zerofit/tests/common/builder.rs` or with `encode::Encoder`.

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy -p zerofit --all-targets --no-default-features -- -D warnings
cargo clippy -p zerofit-analytics --all-targets --no-default-features -- -D warnings
cargo test --workspace --all-features
cargo test -p zerofit --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p zerofit --no-default-features --target thumbv7em-none-eabihf
cargo build -p zerofit-profile --target thumbv7em-none-eabihf
cargo build -p zerofit-analytics --no-default-features --target thumbv7em-none-eabihf
cargo build -p zerofit-fueling --target thumbv7em-none-eabihf
cargo +1.85 check -p zerofit --all-features            # MSRV
cargo +1.85 check -p zerofit-profile
cargo +1.85 check -p zerofit-analytics --all-features
cargo +1.85 check -p zerofit-fueling --all-features
cargo xtask codegen --check                            # generated code up to date
# WebAssembly (wasm-bindgen-cli 0.2.129, matching the pinned crate):
cargo build -p zerofit-wasm --profile wasm-release --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir target/wasm-pkg target/wasm32-unknown-unknown/wasm-release/zerofit_wasm.wasm
node crates/zerofit-wasm/tests/smoke.mjs
# Web app (needs the above toolchain; node 22+):
cd web && npm ci && npm run prepare-assets && npm run check && npm run build && npm run test:e2e
cargo publish --dry-run --workspace                    # never without --dry-run

# Regenerate the profile subset from a new SDK, then the code:
FIT_PROFILE_XLSX=.../FitSDKRelease_21.171.00/Profile.xlsx cargo xtask extract
# Ground truth for fixtures (needs java):
FIT_CSV_TOOL=.../FitSDKRelease_21.171.00/java/FitCSVTool.jar cargo xtask expected
# Benchmarks (quiet machine):
cargo bench -p zerofit-bench --bench decode
cargo bench -p zerofit-bench --bench alloc_count
cargo bench -p zerofit-bench --bench analytics
# Fuzzing (nightly, run under WSL/Linux):
cd fuzz && cargo +nightly fuzz run decode -- -max_total_time=60
```

On Windows PowerShell, set the doc flag with `$env:RUSTDOCFLAGS="-D warnings"`.

## Licensing

Code is dual MIT/Apache-2.0. FIT is Garmin's protocol, and the FIT SDK license
restricts redistribution of the SDK and derivatives. All Profile.xlsx-derived
content (`codegen/profile-subset.json`, `src/generated/`) is isolated in
`zerofit-profile` with a notice in that crate's README; the subset is
excluded from the published package.
