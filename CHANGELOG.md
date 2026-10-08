# Changelog

All notable changes to the `zerofit`, `zerofit-profile` and
`zerofit-analytics` crates. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
crates follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### zerofit-analytics (new)

- `resample`: raw records to a 1 Hz stream over recording time, with
  documented and tested rules for duplicate timestamps, backwards
  timestamps, pauses (gaps over 30 s), short gaps (interpolated), field
  dropouts (repaired up to 8 s), power spikes and optional timer events;
  a `ResampleReport` of what was done.
- Power: average, work, NP (exact integer rolling sums), IF, TSS over
  moving or recording time, VI, time in zones.
- Heart rate: hrTSS (normalized Banister TRIMP), efficiency factor,
  Pa:HR decoupling, time in zones.
- `PowerCurve`: the exact mean-maximal power for every duration in
  O(n²/2), vectorized; `mmp_at` for selected durations; `envelope`.
- `cp`: 2-parameter (work-time regression) and Morton 3-parameter CP/W'
  fits with R², RMSE and standard errors.
- `wbal`: Skiba's differential W' balance, with recovery during pauses.
- `load`: CTL/ATL/TSB (intervals.icu and TrainingPeaks conventions),
  `SeasonCurve`, `estimate_ftp`.
- `workout`: structured workouts (steady and ramp steps in % FTP), planned
  NP/IF/TSS/kJ from the same code as recorded rides, Zwift `.zwo` export
  and FIT workout export (feature `fit`), checked with FitCSVTool.
- `analyze_stream` / `analyze_records` / `analyze_fit` (feature `fit`):
  every metric in one `ActivitySummary` (`Serialize` with feature
  `serde`).
- `#![no_std]` + `alloc`, panic-free lints, builds for
  `thumbv7em-none-eabihf` and `wasm32-unknown-unknown`. MSRV 1.85.
- Example `ride-report`; criterion benchmarks; validation against values
  intervals.icu writes into its FIT export; proptest invariants.
- `zerofit-wasm` (unpublished; first named `zerofit-analytics-wasm`): wasm-bindgen
  `analyze(bytes, settings_json)` with a node smoke test in CI.

## [0.1.0]

First release.

### zerofit

- Zero-copy slice `Decoder`: an `Iterator` over FIT records that borrow the
  input, with no heap allocation. 12- and 14-byte headers, header and file
  CRC-16, 16 local message types, per-definition byte order, all 17 base
  types and their invalid sentinels, arrays and strings, compressed
  timestamp headers with rollover, developer fields, chained files.
- `ReadDecoder` (`std`): the same decoding over any `std::io::Read` with a
  bounded buffer; shares the record parser with `Decoder`, so errors and
  offsets are identical. I/O errors are reported in a separate `ReadError`.
- `encode::Encoder` (`alloc`): a minimal writer that fills in data sizes and
  CRCs and re-encodes decoded records byte for byte.
- `Error` is `Copy` plain data: an `ErrorKind` and the absolute byte offset.
- `#![no_std]` and panic-free by construction: lints deny `unwrap`,
  `expect`, `panic!`, indexing and unchecked arithmetic; `unsafe` is
  forbidden. Builds for `thumbv7em-none-eabihf`. MSRV 1.85.
- Examples: `fit-anonymize` (moves GPS tracks, clears serial numbers, drops
  personal messages, re-encodes with valid CRCs).

### zerofit-profile

- Typed views generated from FIT SDK profile 21.171 for `file_id`, `record`,
  `lap`, `session`, `event`, `device_info`, `activity` and `hrv`: named
  accessors with scale and offset applied, units in the docs, Rust enums for
  enumerated types, field metadata (`message_info`), and `message_name` for
  every message in the profile.
- `developer::DeveloperData` (`alloc`): resolves developer fields through
  the file's `field_description` messages.
- Example: `fit-dump` (FIT to JSON or FitCSVTool-style CSV).

[Unreleased]: https://github.com/ondemous/zerofit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ondemous/zerofit/releases/tag/v0.1.0
