# zerofit

A zero-copy, `no_std`, panic-free decoder for FIT activity files: the binary
format exported by GPS bike computers, running watches and other fitness
devices.

> **Status:** early development (Milestone 1: raw protocol layer). Not yet
> published on crates.io.

## Goals

- **Zero-copy streaming:** an iterator over `&[u8]` that yields messages
  borrowing from the input, with no heap allocation.
- **`no_std` core:** runs on embedded targets; `alloc`/`std` are optional.
- **Never panics on malformed input:** enforced by lints (`unwrap`, `expect`,
  `panic`, slice indexing and unchecked arithmetic are denied in library code)
  and by continuous fuzzing.
- **Precise errors:** every error carries the byte offset where it occurred.

## Crates

| Crate     | Purpose                                                      |
|-----------|--------------------------------------------------------------|
| `zerofit` | Raw protocol layer: headers, CRC, definition and data messages |

A typed profile layer (named messages and fields, generated from the FIT SDK
profile) is planned for a later milestone.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

FIT is a protocol owned by Garmin. This project is not affiliated with or
endorsed by Garmin.
