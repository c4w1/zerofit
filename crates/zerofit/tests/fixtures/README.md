# Fixtures

Real FIT files (`<name>.fit`) with ground truth (`<name>.expected.json`),
checked by `tests/fixtures.rs` (raw layer) and
`../../../zerofit-profile/tests/fixtures.rs` (profile layer).

| File | Source | Notes |
|---|---|---|
| `icu_intervals.fit` | intervals.icu export of a ride | 11k records, 17 laps, developer data id |
| `icu_laps.fit` | intervals.icu export | 4k records, 6 laps |
| `icu_short.fit` | intervals.icu export | 90 records; quick to debug |
| `wahoo_elemnt.fit` | Wahoo ELEMNT BOLT recording | profile 20.27, 15 developer fields, segment laps, manufacturer-specific messages |

Rules:

- Only the maintainer's own recordings. Never Garmin FIT SDK sample files.
- Every file is anonymized before it is committed (GPS moved to 0°N 140°W,
  serial numbers cleared, `user_profile` dropped):

  ```sh
  cargo run -p zerofit --example fit-anonymize -- original.fit crates/zerofit/tests/fixtures/<name>.fit
  ```

- Expected values come from Garmin's FitCSVTool, never from zerofit:

  ```sh
  FIT_CSV_TOOL=/path/to/FitSDKRelease_21.171.00/java/FitCSVTool.jar cargo xtask expected
  ```

  This writes header fields and CRC validity (from a separate minimal
  implementation in `xtask`), message counts, first/last spot checks with raw
  field values for the profile messages, and the scaled values of the first
  `session` with tolerances.
