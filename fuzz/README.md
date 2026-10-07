# Fuzzing

Requires nightly Rust and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz);
runs best on Linux (or WSL).

```sh
cargo install cargo-fuzz
cd fuzz
mkdir -p corpus/decode && cp seeds/* corpus/decode/
cargo +nightly fuzz run decode -- -max_total_time=1800
```

`seeds/` holds small synthetic FIT files (generated for this project, not taken
from the FIT SDK) covering 12/14-byte headers, both byte orders, compressed
timestamps, developer fields and chained files. Crashes land in
`artifacts/decode/`; minimize one with `cargo +nightly fuzz tmin decode <file>`
and add it as a regression test.
