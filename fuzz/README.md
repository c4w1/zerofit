# Fuzzing

Requires nightly Rust and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz);
runs on Linux or WSL (libFuzzer does not build with MSVC).

| Target | Checks |
|---|---|
| `decode` | The slice `Decoder`, with and without CRC validation: no panic or hang, every step advances, error offsets are inside the input, fused after an error. Every field value is decoded. |
| `differential` | `ReadDecoder` (reader returning 1-64 bytes per call, chosen by the first input byte) produces exactly the records and errors of `Decoder`. |
| `roundtrip` | Decoded records re-encode (`encode::Encoder`) to a file that decodes with CRCs checked to the same definitions and data. Also runs the profile layer and developer field resolution on every message. |

```sh
cargo install cargo-fuzz
cd fuzz
for t in decode differential roundtrip; do mkdir -p corpus/$t && cp seeds/* corpus/$t/; done
cargo +nightly fuzz run -O decode -- -max_total_time=1800
# all three in parallel, 4 workers each, keep going after a crash:
cargo +nightly fuzz run -O roundtrip -- -fork=4 -ignore_crashes=1 -max_total_time=3600
```

`seeds/` holds small synthetic FIT files (generated for this project, not taken
from the FIT SDK) covering 12/14-byte headers, both byte orders, compressed
timestamps, developer fields and chained files. Seeding with the first few KiB
of each fixture in `crates/zerofit/tests/fixtures/` also helps.

## Regression corpus

`crates/zerofit/tests/fuzz_corpus/` holds a minimized corpus that
`crates/zerofit/tests/fuzz_corpus.rs` replays through the same invariants on
every `cargo test`. Any crashing input goes there too. To refresh it after a
long run:

```sh
cargo +nightly fuzz cmin -O decode
cp corpus/decode/* ../crates/zerofit/tests/fuzz_corpus/   # keep files under 4 KiB
```

Crashes land in `artifacts/<target>/`; minimize with
`cargo +nightly fuzz tmin -O <target> <file>`, fix, and add the input to the
regression corpus.

## Results

Campaign on 2026-10-07 (Intel Core Ultra 7 165U, WSL2, nightly 1.101), one
hour per target with `-fork=4`, seeded with `seeds/` and the fixtures' first
4 KiB:

| Target | Executions | Coverage (edges) | Corpus | Crashes |
|---|---:|---:|---:|---:|
| `decode` | 160.8 M | 472 | 450 | 0 |
| `differential` | 10.7 M | 559 | 282 | 0 |
| `roundtrip` | 8.4 M | 1501 | 2325 | 0 |

The only failure found was in a fuzz *harness*: the first `roundtrip` run
crashed on the empty input because the target expected any re-encoded output
to decode, but an empty input produces an empty output, which is (correctly)
an error. The library itself produced no crash, hang, divergence between the
two decoders, or encode/decode asymmetry. Coverage plateaued within the first
15 minutes of each run.
