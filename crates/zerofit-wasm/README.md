# zerofit-wasm (internal)

All three zerofit crates compiled to one WebAssembly module for the web app
in `web/`: decoding and analysis (`zerofit`, `zerofit-profile`,
`zerofit-analytics`), fitness across activities, structured workouts, and
fueling plans (`zerofit-fueling`). Not published.

| Export | In | Out |
|---|---|---|
| `analyze` | FIT bytes, settings JSON | `ActivityResult`: summary JSON + typed-array streams + power curve on a log grid |
| `analyze_summary` | FIT bytes, settings JSON | summary JSON |
| `fitness` | activities (day, TSS, curve) JSON | CTL/ATL/TSB series, season curve, CP fits, eFTP |
| `plan_workout` | workout JSON, FTP | planned duration/NP/IF/TSS/kJ |
| `export_zwo` / `export_fit_workout` | workout JSON | `.zwo` XML / FIT bytes |
| `fueling_day` | athlete + sessions JSON | day plan JSON |

Each export wraps a plain Rust function (`analyze_activity`, `*_json`) that
is unit-tested natively; `tests/smoke.mjs` checks every export in node and
`tests/bench.mjs` times `analyze`.

```sh
cd web && npm run wasm     # cargo build --profile wasm-release + wasm-bindgen --target web + wasm-opt -Oz
```

## Size and speed

Measured on 2026-10-08. The time is node 24 on a Core Ultra 7 165U, analyzing the
synthetic 4-hour ride (`cargo run -p zerofit-bench --example synthetic-fit`):
decode, resample, every metric, the full power curve and W'bal. It is the
median of 15 runs. Sizes are the wasm-bindgen output.

| Build | Raw | Gzip | 4 h analysis |
|---|---|---|---|
| `--release` defaults | 364 KB | 123 KB | 162 ms |
| defaults + `wasm-opt -Oz` | 267 KB | 116 KB | 148 ms |
| `lto = "fat"`, `codegen-units = 1` | 327 KB | 119 KB | 160 ms |
| **LTO + `wasm-opt -Oz` (shipped: `profile.wasm-release`)** | **255 KB** | **110 KB** | **149 ms** |
| LTO + `opt-level = "s"` + `wasm-opt -Oz` | 229 KB | 100 KB | 192 ms |
| LTO + `opt-level = "z"` + `wasm-opt -Oz` | 222 KB | 99 KB | 208 ms |

The shipped build is 30 % smaller than the defaults (11 % gzipped) and
8 % faster. The size-optimizing LLVM levels would save another ~10 KB
gzipped, but the vectorized power-curve loop runs 29–40 % slower; every
uploaded ride runs through it, so speed wins. `panic = "abort"` is already the
default for `wasm32-unknown-unknown`.

`unsafe_code` is `deny` here, not `forbid`: `#[wasm_bindgen]` expands to FFI glue, which
is allowed with a documented `#[allow]` on the exports only.
