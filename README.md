# zerofit

A zero-copy, `no_std`, panic-free decoder for FIT activity files: the binary
format exported by GPS bike computers, running watches and other fitness
devices.

> **Status:** Milestone 1 (raw protocol layer) is implemented. Not yet published
> on crates.io. Typed profile messages (`record`, `lap`, `session`, ...), a
> `std::io::Read` decoder, benchmarks and a CLI are next.

```rust
use zerofit::{Decoder, Record, Value};

let bytes = std::fs::read("ride.fit")?;
for record in Decoder::new(&bytes) {
    if let Record::Data(msg) = record? {
        // Global message 20 is `record`; field 3 is `heart_rate`.
        if msg.global_message_number() == 20 {
            if let Some(Value::UInt8(bpm)) = msg.field(3).and_then(|f| f.value()) {
                println!("{:?}: {bpm} bpm", msg.timestamp());
            }
        }
    }
}
```

## Why another FIT crate?

- **Zero-copy streaming.** `Decoder` is an `Iterator` over `&[u8]` whose
  messages borrow from the input. Definitions are kept as borrowed slices in a
  fixed 16-slot table, so decoding performs no heap allocation. Field values
  are decoded lazily, only when you ask for them.
- **`no_std` core.** Builds for bare-metal targets (CI checks
  `thumbv7em-none-eabihf`); `alloc`/`std` are optional.
- **Never panics on malformed input.** Enforced by lints, not code review:
  library code denies `unwrap`, `expect`, `panic!`, slice indexing and
  unchecked arithmetic. Backed by property tests and `cargo-fuzz`.
- **Precise errors.** Every error carries the absolute byte offset where
  decoding stopped, and is `Copy` plain data.

## Protocol coverage

| Feature                                          | Status |
|--------------------------------------------------|:------:|
| 12- and 14-byte file headers                     | ✅ |
| Header CRC and file CRC-16 validation (optional) | ✅ |
| Definition messages, 16 local message types      | ✅ |
| Per-definition byte order                        | ✅ |
| All 17 base types, arrays, strings, sentinels    | ✅ |
| Compressed timestamp headers (with rollover)     | ✅ |
| Developer data fields (raw bytes)                | ✅ |
| Chained FIT files in one stream                  | ✅ |
| Developer field descriptions (typed)             | planned |
| Typed profile layer                              | planned |
| `std::io::Read` streaming decoder                | planned |

Like the FIT SDK, a field whose size is not a multiple of its base type's size,
or whose base type is unknown, is exposed as raw bytes instead of failing the
whole file.

## `no_std` tradeoffs

| Constraint | Consequence |
|---|---|
| No `std::io` in core | The `Read`-based decoder will sit behind the `std` feature; `no_std` users decode slices |
| No `alloc` | Definitions must borrow the input, so the zero-allocation API works on complete buffers |
| Errors can't hold `io::Error`/`String` | The core `Error` is plain `Copy` data (kind + byte offset) |
| No `HashMap` | Fixed arrays; resolving developer field descriptions will need `alloc` |
| No float functions like `round` in `core` | Profile scaling will use only `*`, `+`, `/` |

## Development

See [CLAUDE.md](CLAUDE.md) for architecture, conventions and the full list of
check commands. Quick version:

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p zerofit --no-default-features --target thumbv7em-none-eabihf
```

Real-file tests live in `crates/zerofit/tests/fixtures/` (see the README
there); fuzzing instructions are in [`fuzz/README.md`](fuzz/README.md).

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
implementation of the publicly documented protocol and is not affiliated with
or endorsed by Garmin. It does not include the FIT SDK or files from it. Use of
the FIT protocol may be subject to Garmin's
[FIT Protocol License](https://developer.garmin.com/fit/); you are responsible
for complying with it.
