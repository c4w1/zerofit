# Changelog

All notable changes to the `zerofit` and `zerofit-profile` crates. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
crates follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
