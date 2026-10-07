# zerofit

A zero-copy, `no_std`, panic-free Rust decoder for FIT activity files, the
binary format written by Garmin, Wahoo and other bike computers and watches.
`zerofit` decodes a file straight out of the byte buffer it already lives in:
no allocation, no copying, field values decoded only when you ask for them.
It builds for bare-metal ARM, reports every malformed input as a precise
error with a byte offset instead of panicking, and is checked against
Garmin's own FitCSVTool on real recordings and by continuous fuzzing. On real recordings it is 20-28x faster than the `fitparser` crate at
fully decoding a file, and allocates nothing.

```rust
use zerofit::{Decoder, Record};
use zerofit_profile::Message;

let bytes = std::fs::read("ride.fit")?;
for record in Decoder::new(&bytes) {
    if let Record::Data(msg) = record? {
        match Message::new(msg) {
            Message::Record(r) => println!("{:?} bpm, {:?} m/s", r.heart_rate(), r.speed()),
            Message::Session(s) => println!("{:?} m in {:?} s", s.total_distance(), s.total_timer_time()),
            _ => {}
        }
    }
}
```

| Crate | What it is |
|---|---|
| [`zerofit`](crates/zerofit) | The FIT protocol: headers, CRCs, definitions, data messages, base types. Slice decoder, streaming decoder, minimal encoder. |
| [`zerofit-profile`](crates/zerofit-profile) | The FIT profile, generated from the FIT SDK: typed `record`, `lap`, `session`, ... with scaling, units and enums; developer fields. |

## Features

- **Zero-copy, zero-allocation decoding.** `Decoder` is an `Iterator` over a
  `&[u8]`; every message borrows from it. Definitions live in a fixed
  16-slot table. Field values are decoded lazily with the right byte order.
- **Streaming.** `ReadDecoder` decodes from any `std::io::Read` with a
  bounded buffer (64 KiB plus the largest record), using the same record
  parser, so both decoders report identical errors at identical offsets.
- **Complete protocol coverage.** 12- and 14-byte headers, header and file
  CRC-16, both byte orders, all 17 base types with invalid sentinels,
  arrays, strings, compressed-timestamp headers with rollover, developer
  fields, chained files.
- **Typed profile layer.** Generated from the FIT SDK's `Profile.xlsx` for
  `file_id`, `record`, `lap`, `session`, `event`, `device_info`, `activity`
  and `hrv`: `record.speed()` returns metres per second as `f64`,
  `session.sport()` a `Sport` enum. Developer fields (Connect IQ data
  fields) are resolved by name and units.
- **Never panics.** Library code is compiled with lints that deny
  `unwrap`, `expect`, `panic!`, slice indexing and unchecked arithmetic;
  `unsafe` is forbidden.
- **Precise errors.** `Error` is `Copy` plain data: what went wrong and the
  absolute byte offset.
- **Encoder.** Rewrite a decoded file with correct CRCs, byte for byte.
- **Tools.** `fit-dump` (FIT to JSON or CSV) and `fit-anonymize` (move GPS
  tracks, clear serial numbers, re-encode).

```sh
cargo run -p zerofit-profile --example fit-dump -- --csv ride.fit
cargo run -p zerofit --example fit-anonymize -- ride.fit shareable.fit
```

## `no_std`

With `default-features = false`, `zerofit` is `#![no_std]`, never allocates
and builds for bare-metal targets (CI checks `thumbv7em-none-eabihf`).
`zerofit-profile` is `no_std` too.

| Constraint | Consequence |
|---|---|
| No `std::io` in core | `ReadDecoder` is behind the `std` feature; `no_std` users decode slices |
| No `alloc` | Definitions borrow the input, so the zero-allocation API needs the file (or each chained file) in one buffer |
| Errors can't hold `io::Error` or `String` | The core `Error` is `Copy` data (kind and byte offset); I/O errors live in the std-only `ReadError` |
| No `HashMap` | Fixed arrays; developer-field resolution needs `alloc` (`BTreeMap`) |
| No float functions such as `round` in `core` | Profile scaling uses only `/` and `-` |

Features: `zerofit/std` (default; `ReadDecoder`, implies `alloc`),
`zerofit/alloc` (`encode::Encoder`), `zerofit-profile/alloc`
(`developer::DeveloperData`).

## Performance

Measured with `cargo bench -p zerofit-bench` (criterion, median of 20
samples) on an Intel Core Ultra 7 165U laptop (Windows 11, AC power),
`rustc 1.98.1`, release profile, files already in memory. Throughput in MiB/s
of FIT file; messages per second in parentheses.

| Fixture | Size | Messages | zerofit: iterate records | zerofit: decode every field | zerofit: decode + profile-scale every field | zerofit: streaming | fitparser 0.11 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `icu_intervals` | 334 KiB | 11,362 | 816 (23.1 M/s) | 86.6 (2.67 M/s) | 68.4 (2.08 M/s) | 503 (18.8 M/s) | 2.42 (106 K/s) |
| `icu_laps` | 116 KiB | 4,240 | 734 (20.8 M/s) | 103 (3.53 M/s) | 78.8 (2.45 M/s) | 493 (17.9 M/s) | 3.24 (114 K/s) |
| `icu_short` | 2.5 KiB | 96 | 641 (21.7 M/s) | 145 (4.94 M/s) | 80.9 (3.95 M/s) | 215 (9.7 M/s) | 3.97 (184 K/s) |
| `wahoo_elemnt` | 933 KiB | 25,064 | 856 (23.1 M/s) | 121 (3.11 M/s) | 77.0 (2.10 M/s) | 515 (14.7 M/s) | 2.72 (78 K/s) |

The fair comparison with fitparser, which names and scales every field into
owned values, is the "decode + profile-scale" column: **20-28x faster**.
Iterating records (what you pay to find, say, the one `session` message) is
**160-340x faster**. All zerofit columns include the CRC check.

Heap allocations per decode (`cargo bench -p zerofit-bench --bench alloc_count`):

| Fixture | zerofit (slice, any workload) | zerofit (streaming) | fitparser |
|---|---:|---:|---:|
| `icu_intervals` | **0** | 8 (197 KB) | 580,800 (84 MB) |
| `icu_laps` | **0** | 6 (197 KB) | 182,524 (24 MB) |
| `icu_short` | **0** | 5 (66 KB) | 3,176 (0.37 MB) |
| `wahoo_elemnt` | **0** | 6 (197 KB) | 1,581,064 (310 MB) |

Three optimizations came out of profiling, each explained where it is
implemented: slicing-by-8 CRC (`Crc16::update`, record iteration 2.2-3.4x),
`#[inline]` on the per-field decode path (`Value::decode`, field decoding
2-4x), and a generated field-number index for profile metadata
(`MessageInfo::field`, up to 1.26x). A laptop CPU with hybrid cores gives run-to-run
variance of roughly ±10%; the ratios are stable.

## How correctness is verified

- **Ground truth from Garmin's tools, not ourselves.** Every fixture's
  expected values (`<name>.expected.json`) are produced by
  `cargo xtask expected`, which runs the FIT SDK's FitCSVTool and records
  message counts, raw field values of the first and last instance of each
  profile message, and the scaled session values. Header fields and CRC
  validity come from a separate 40-line implementation in `xtask`. `xtask`
  cannot depend on `zerofit` (a test enforces it). Both layers are tested
  against these files: counts, CRCs, 2 spot checks per message type and
  every scaled session value FitCSVTool prints.
- **Real recordings.** Three intervals.icu exports and a Wahoo ELEMNT BOLT
  file with developer fields and manufacturer-specific messages,
  anonymized with `fit-anonymize`.
- **Generated error cases.** 96 corruptions derived from the fixtures
  (truncation at every structural stage, flipped bytes, bad signature,
  undefined local message type, invalid architecture, data-size overrun),
  each with its exact expected error kind and byte offset computed by an
  independent record walker, for both decoders.
- **Fuzzing.** Three `cargo-fuzz` targets: the decoder's invariants (no
  panic, progress, offsets in bounds, fused after error), differential
  (streaming decoder vs slice decoder at random chunk sizes), and
  decode → encode → decode round trips through the profile layer.
  A one-hour campaign per target (179 million executions in total) found
no crash in the library. The minimized corpus is replayed by `cargo test`.
- **Property tests.** CRC against the SDK's reference algorithm, every base
  type in both byte orders, the encoder's round trips, and the streaming
  decoder against the slice decoder on random input.
- **CI.** fmt, clippy (pedantic, `-D warnings`), tests on Linux, macOS and
  Windows, `no_std` builds for `thumbv7em-none-eabihf`, MSRV 1.85, docs with
  `-D warnings`, generated code up to date, and a fuzz smoke test.

## Development

See [CLAUDE.md](CLAUDE.md) for architecture, conventions and every check
command, [docs/INTERVIEW.md](docs/INTERVIEW.md) for a design walkthrough,
and [`fuzz/README.md`](fuzz/README.md) for fuzzing.

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo xtask codegen --check
cargo bench -p zerofit-bench
```

Minimum supported Rust version: **1.85**.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

### FIT protocol notice

FIT is a protocol owned by Garmin. This project is an independent
implementation and is not affiliated with or endorsed by Garmin. It does not
include the FIT SDK. The generated code in `zerofit-profile` is derived from
the SDK's `Profile.xlsx`; see that crate's [README](crates/zerofit-profile/README.md)
for the license notice that applies to it. Use of the FIT protocol may be
subject to Garmin's [FIT Protocol License](https://developer.garmin.com/fit/).
