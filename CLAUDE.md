# CLAUDE.md

Guidance for working on **zerofit**, a zero-copy, `no_std`, panic-free Rust
decoder for FIT activity files. This is a portfolio project aimed at
systems/embedded roles: correctness, performance, tests and docs matter more
than feature count.

## Status and scope

- **Milestone 1 (current):** workspace, header parsing + CRC validation,
  definition and data messages in the raw layer, fixture test harness,
  fuzz target, CI.
- **Milestone 2:** `std::io::Read` streaming decoder, profile codegen, typed
  profile layer (`file_id`, `record`, `lap`, `session`, `event`,
  `device_info`, `activity`, `hrv`), developer field resolution.
- **Milestone 3:** criterion benchmarks vs `fitparser`, CLI example
  (FIT → JSON/CSV).
- **Out of scope for now:** training-analytics crate, web app. Do not start
  these.

## Crate map

```
crates/zerofit/          raw protocol decoder, #![no_std], features: alloc, std
crates/zerofit-profile/  (M2) typed profile layer, generated code only
crates/zerofit-codegen/  (M2) publish = false; Profile.xlsx -> Rust source
fuzz/                    cargo-fuzz crate, own workspace, nightly only
```

## Architecture

- **Two layers.** The raw layer (`zerofit`) knows only the protocol: file
  header, CRC-16, record headers, definition messages, data messages, base
  types and invalid sentinels. It never names a message or field, with one
  exception: field 253 (`timestamp`) is protocol-level because compressed
  timestamp headers depend on it. The profile layer maps global message and
  field numbers to names, scale/offset, units and enums.
- **Zero-copy definitions.** A definition message's field list is kept as a
  borrowed `&'a [u8]` of 3-byte triples, so `Definition<'a>` is `Copy`. The
  decoder holds a fixed table of 16 local-message slots (no heap).
  `DataMessage<'a>` carries a copy of its definition, so the slice decoder is
  a plain `Iterator` whose items borrow only the input.
- **Lazy decoding.** Iterating messages costs only record-header parsing and a
  slice split. Field values are decoded (with per-definition endianness) on
  demand. Message size and the offset of field 253 are precomputed when a
  definition is parsed.
- **Errors.** `Error { offset: u64, kind: ErrorKind }` is `Copy + PartialEq`.
  The offset is the absolute byte position in the input stream. After the
  first error the decoder is fused (returns `None`). I/O errors (M2) live in a
  separate std-only error type.
- **Stream readiness.** Core parse functions report "need N more bytes"
  instead of failing, so the M2 `Read`-based decoder is a buffer-refill wrapper
  around the same code.
- **Chained files.** After a file's CRC, if bytes remain, a new header is
  parsed and the definition table is reset.

## no_std tradeoffs

| Constraint | Consequence |
|---|---|
| No `std::io` in core | `Read` API behind `std`; no_std users feed slices |
| No `alloc` | Only the slice API; definitions borrow the input; fixed 16-slot table |
| Errors can't hold `io::Error`/`String` | Core error is plain `Copy` data |
| No `HashMap` | Fixed arrays; developer-field resolution needs `alloc` |
| No float math like `round` in `core` | Profile scaling uses only `*`, `+`, `/` |

## Invariants (non-negotiable)

- **Never panic on malformed input.** Library code must not use `unwrap`,
  `expect`, `panic!`, slice indexing (`a[i]`, `a[i..j]`) or unchecked
  arithmetic. These are denied by workspace lints. Use `get`, `split_at_checked`,
  `checked_*`/`wrapping_*`, and return `Error`s with byte offsets.
- **No `unsafe`** (`unsafe_code = "forbid"`).
- **Every public item has rustdoc** (`missing_docs = "deny"`), with a runnable
  example for types and functions users call directly.
- **The core must build for `thumbv7em-none-eabihf` with
  `--no-default-features`.** Never use `std::` in core code; use `core::` or
  `alloc::` behind the feature.
- Test code may use `unwrap`/indexing: unit tests are exempted in
  `clippy.toml`; integration test crates start with a crate-level
  `#![allow(...)]`.

## Conventions

- **Small commits, one logical change each.** Run the full check suite (below)
  before every commit, then report what changed and what's next.
- **Ask before adding any dependency.** Approved so far: `thiserror` (no_std,
  `default-features = false`), `proptest` (dev), `serde` + `serde_json` (dev,
  fixtures), `criterion` + `fitparser` (dev, M3 benches), `libfuzzer-sys` (fuzz
  crate). Anything else needs approval.
- **Never commit Garmin SDK files:** no `Profile.xlsx`, no SDK sample `.fit`
  files, no FIT SDK source. Fixtures must be the user's own recordings.
- MSRV is **1.85** (edition 2024). Don't use newer std APIs without checking.
- Tests: unit tests next to the code (`#[cfg(test)] mod tests`); integration
  tests in `crates/zerofit/tests/`. Synthetic FIT files are built with the
  dev-only writer in `tests/common/builder.rs`.

## Commands

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test -p zerofit --no-default-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo build -p zerofit --no-default-features --target thumbv7em-none-eabihf
cargo +1.85 check -p zerofit --all-features            # MSRV
# Fuzzing (nightly, run under WSL/Linux):
cd fuzz && cargo +nightly fuzz run decode -- -max_total_time=60
```

On Windows PowerShell, set the doc flag with `$env:RUSTDOCFLAGS="-D warnings"`.

## Fixtures

`crates/zerofit/tests/fixtures/<name>.fit` plus `<name>.json` with expected
values. The harness (`tests/fixtures.rs`) runs over every `.fit` file and skips
cleanly when the directory is empty. JSON schema:

```json
{
  "header": {"size": 14, "protocol_version": 32, "profile_version": 2132, "data_size": 123456},
  "header_crc_valid": true,
  "file_crc_valid": true,
  "files_in_stream": 1,
  "definitions": 42,
  "data_messages": 3701,
  "by_global_message": {"0": 1, "20": 3600},
  "spot_checks": [{"index": 5, "global": 20, "fields": {"253": 1011234567}}],
  "expect_error": null
}
```

Expected counts are cross-checked against an independent decoder before being
committed; spot checks come from Garmin's FitCSVTool output (run locally, not
redistributed).

## Licensing

Code is dual MIT/Apache-2.0. FIT is Garmin's protocol, and the FIT SDK license
restricts redistribution of the SDK and derivatives. Keep all Profile.xlsx-derived
code isolated in `zerofit-profile` with a notice in that crate's README.
