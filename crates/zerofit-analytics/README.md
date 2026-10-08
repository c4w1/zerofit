# zerofit-analytics

Training analytics for 1 Hz activity streams: normalized power, IF, TSS,
VI, work, hrTSS, efficiency factor, Pa:HR decoupling, time in zones, the
mean-maximal power curve, critical power / W' fits, W' balance, and
across activities CTL/ATL/TSB, season power curves and estimated FTP.

- **Pure library.** No I/O, `#![no_std]` + `alloc`, panic-free under the
  same lints as the `zerofit` decoder (no `unwrap`, indexing or
  unchecked arithmetic). Builds for `thumbv7em-none-eabihf` and
  `wasm32-unknown-unknown`.
- **Every formula documented.** Each metric's rustdoc gives the formula,
  its source (Coggan, Banister, Skiba, Monod & Scherrer, Morton, Friel)
  and the judgment calls, such as zeros, pauses, the first 30 seconds and
  which duration TSS uses.
- **intervals.icu conventions by default.** Where intervals.icu differs
  from the textbook, the default follows intervals.icu, the difference
  is documented, and the textbook variant is a setting.
- **Validated** against intervals.icu (below), with hand-computed unit
  tests and property tests.

```rust
use zerofit_analytics::{AnalysisConfig, AthleteSettings, analyze_fit};

let bytes = std::fs::read("ride.fit")?;
let athlete = AthleteSettings { ftp: Some(250.0), lthr: Some(165), ..AthleteSettings::default() };
let analysis = analyze_fit(&bytes, &athlete, &AnalysisConfig::default())?;
let s = &analysis.summary;
println!("NP {:?} W, IF {:?}, TSS {:?}", s.normalized_power, s.intensity_factor, s.tss);
```

Or from the command line:

```sh
cargo run -p zerofit-analytics --example ride-report -- ride.fit --ftp 250 --lthr 165 --max-hr 188 --rest-hr 48
```

## The resampler: why it matters

Every metric is a sum or rolling window over one sample per second, so
how raw records become that series decides the answer. The rules, each
documented in `resample` and unit-tested:

| Situation | Treatment (default) |
|---|---|
| Duplicate timestamps | merged field by field, later record wins |
| Timestamp goes backwards | record dropped |
| No records for > 30 s | **pause**: removed (stream is recording time; elapsed time kept per sample) |
| No records for 1–30 s | **filled**: every channel linearly interpolated |
| Field missing ≤ 8 recorded s | repaired by interpolation (held at segment edges) |
| Field missing > 8 s | power → 0 W, flagged; HR etc. → missing, excluded from averages |
| Power > 2500 W | spike, treated as a dropout |
| Timer stop/start events | ignored by default (auto-pause already shows as a gap); optional |

The 30 s and 8 s limits are intervals.icu's (developer posts: recording
time is elapsed time with gaps "of more than 30s removed"; the
`fixed_watts` stream has "drop outs (8s or less) fixed"). On the fixture
ride `icu_laps`, the work of our resampled stream equals the
`total_work` intervals.icu wrote into its export to the joule
(805,907 J), which is evidence that the series is the same.

## Metrics

| Metric | Formula | Source | Notable judgment call |
|---|---|---|---|
| Average power | ΣP / n | — | zeros count; pauses don't |
| NP | (mean of 30 s rolling avg⁴)^¼ | Coggan | first full window at 30 s; windows span pauses |
| IF | NP / FTP | Coggan | |
| TSS | t · IF² · 100 / 3600 | Coggan | **t = moving time** (intervals.icu); recording time optional |
| VI | NP / avg | Coggan | |
| Work | ΣP · 1 s | — | kJ |
| hrTSS | TRIMP / TRIMP(1 h at LTHR) · 100 | Banister TRIMP (HRSS) | HR-reserve fraction clamped to 0–1; moving seconds |
| EF | NP / avg HR | Friel | |
| Pa:HR decoupling | (r₁ − r₂)/r₁, r = avg P / avg HR per half | Friel | average power (intervals.icu); positive = HR drift |
| Zones | Coggan 7 power zones (% FTP), Friel 7 HR zones (% LTHR) | | upper bounds inclusive |
| MMP curve | max over windows of avg power, every duration | | over elapsed time with pauses as 0 W (intervals.icu) |
| CP / W' | P·t = CP·t + W' (2P); P = W'/(t−k) + CP (3P) | Monod & Scherrer; Morton | log-spaced 2–20 min points; R², RMSE, SE reported |
| W'bal | depletion P−CP; recovery e^(−(CP−P)/W') | Skiba 2015 (differential) | may go negative; pauses recover at 0 W |
| CTL/ATL/TSB | EWMA τ = 42 / 7 days | Banister, Coggan | e^(−1/τ) and same-day TSB (intervals.icu); 1/τ + yesterday's TSB (TrainingPeaks) optional |
| eFTP | max over 3–30 min efforts of (MMP(d) − W'/d) + W'/3600 | | the effort's point on the athlete's own W' hyperbola, read at 1 h |

Two things the property tests showed, both now in the docs:

- **The exact MMP curve is not always non-increasing.** For
  `[980, 0, 980]`, MMP(2) = 490 W but MMP(3) = 653 W. On the fixture rides
  it rises at thousands of durations, by up to 10 W per step, and sits up
  to 8% below its envelope. What does hold is MMP(k·d) ≤ MMP(d) and that
  best work never decreases with duration. `PowerCurve::envelope` gives
  the monotone curve.
- **NP ≥ average power is not guaranteed at the edges.** One hard second
  followed by 30 s of zeros gives NP ≈ 28 W against an average of 32 W.
  It holds exactly when the ride starts and ends with ≥ 29 s of zeros,
  and NP is always ≥ the mean of its rolling averages.

## Validation against intervals.icu

`tests/fixtures.rs` compares against two independent references:

1. **`tests/fixtures/intervals_icu.json`**: values recorded by hand from
   the intervals.icu web app, with the settings used. *This file is still
   a template (all `null`); fill it in and run
   `cargo test -p zerofit-analytics --test fixtures -- --nocapture` to get
   the full table.*
2. **The session message in intervals.icu's own FIT export**
   (`file_id.product_name = "Intervals.icu"`), with the FTP it used
   (`threshold_power` = 323 W):

| Ride | Metric | zerofit-analytics | intervals.icu | Error | Tolerance |
|---|---|---|---|---|---|
| icu_laps | work | 805.907 kJ | 805.907 kJ | **0.000 %** | ±0.01 % |
| icu_laps | average power | 183.87 W | 184 W | −0.07 % | ±0.6 W (rounding) |
| icu_laps | NP | 254.02 W | 256 W | −0.77 % | ±1 % |
| icu_laps | IF | 0.786 | 0.793 | −0.83 % | ±1 % |
| icu_laps | TSS | 72.50 | 73 | −0.69 % | ±2 % |
| icu_intervals | work | 1778.98 kJ | 1778.86 kJ | +0.007 % | ±0.01 % |
| icu_intervals | average power | 154.52 W | 155 W | −0.31 % | ±0.6 W (rounding) |
| icu_intervals | NP | 204.11 W | 204 W | +0.05 % | ±1 % |
| icu_intervals | IF | 0.632 | 0.632 | −0.01 % | ±1 % |
| icu_intervals | TSS | 125.79 | 124 | **+1.45 %** | ±2 % |

Investigated differences (nothing was tuned to fit):

- **TSS on icu_intervals (+1.45 %)**: NP and IF match to 0.05 %, so the
  whole difference is the duration. intervals.icu computes load over
  *moving time* from the velocity stream, with an unpublished threshold.
  Its TSS implies about 11,190 s moving; ours (speed > 0.5 m/s) is
  11,341 s. With recording time instead, the error would be +3 %. A
  threshold fitted to these two rides disagrees between them, so the
  default stays a round, documented 0.5 m/s.
- **NP on icu_laps (−0.77 %)**: the streams are identical (the work
  matches to the joule), so this is algorithmic. Per-segment windows,
  partial first windows, EWMA smoothing (25/30 s) and excluding zeros
  were all tried; none matches both rides, and the textbook definition
  matches icu_intervals to 0.05 %. Unexplained, and under 1 %.
- **Average power**: intervals.icu's recording time is P + 2 seconds
  shorter than ours for a ride with P pauses (4378 vs 4383 s). This is a
  counting convention at segment edges, about 0.1 %.
- **Wahoo ELEMNT device values** (not asserted): the head unit's
  `total_work` is 1.5 % below the sum of its own per-second power
  records, so device summaries are not a reference.

## Performance

`cargo bench -p zerofit-bench --bench analytics`. Intel Core Ultra 7 165U
laptop, Windows, `x86_64` baseline target (SSE2). The machine was not
idle, so expect ±15 % between runs. Synthetic rides are deterministic:
intervals, sprints, 1 % missing records and hourly stops.

| Workload | Time |
|---|---|
| Full analysis of a 4 h ride (resample + every metric + full MMP curve + CP + eFTP + W'bal) | 55.6 ms |
| Exact MMP curve, every duration 1 s–6 h (21,600 s, 233 M windows) | 75.0 ms (3.1 G windows/s) |
| MMP at 24 standard durations, 6 h | 0.90 ms |
| CTL/ATL/TSB over 3 years (1095 days) | 11.7 µs |
| `analyze_fit` on `icu_laps` (1 h 16 min, 341 KB) | 6.2 ms |
| `analyze_fit` on `wahoo_elemnt` (6 h 51 min elapsed, 955 KB) | 121 ms |

The full analysis is dominated by the exact MMP curve, which is
O(n²/2). Computing the best window sum for *every* length is a (max,+)
convolution, with no known truly subquadratic algorithm, so the work goes
into the constant factor:

- prefix sums in `i32` when the total fits (always, for real rides);
- 16 independent lane maxima over fixed-size chunks;
- a signed 32-bit max, which compiles to an SSE2 compare and blend
  (checked in the assembly).

`mmp_at` computes selected durations in O(n·k).

## Features

- `fit` (default): `fit::read_fit` and `analyze_fit`, via `zerofit` and
  `zerofit-profile`. Without it the crate has no FIT dependency and
  contains no FIT SDK-derived code.
- `serde`: `Serialize` for the summary and result types.

`zerofit-analytics-wasm` (in this repository, unpublished) wraps
`analyze_fit` for WebAssembly: `analyze(fitBytes, settingsJson) →
summaryJson`, with a node smoke test.

## License

MIT OR Apache-2.0. With the `fit` feature, this crate depends on
`zerofit-profile`, whose generated code is derived from the FIT SDK
profile; see that crate's README for the FIT SDK license notice.
