## Fixtures

`crates/zerofit/tests/fixtures/<name>.fit` plus `<name>.expected.json`
written by `cargo xtask expected` (schema: `xtask/src/expected.rs` and
`crates/zerofit/tests/common/expected.rs`):

```json
{
  "generated_by": "cargo xtask expected: FitCSVTool -i -re (FIT SDK profile 21.171)",
  "header": {"size": 14, "protocol_version": 32, "profile_version": 21176, "data_size": 2557},
  "header_crc_valid": true,
  "file_crc_valid": true,
  "files_in_stream": 1,
  "definitions": 8,
  "data_messages": 96,
  "by_global_message": {"0": 1, "20": 90},
  "unknown_messages": 0,
  "spot_checks": [{"global": 20, "message": "record", "occurrence": 0, "fields": {"253": 1066136399, "3": 99}}],
  "profile": {"session": {"total_distance": {"value": 1639.9, "tolerance": 0.005}}}
}
```

Header and CRC validity come from a separate minimal implementation in
`xtask`; counts, raw spot-check values (FitCSVTool output un-scaled with the
profile subset) and session values come from FitCSVTool. Both layers are
tested against these files (`crates/zerofit/tests/fixtures.rs`,
`crates/zerofit-profile/tests/fixtures.rs`), and
`crates/zerofit/tests/error_fixtures.rs` derives error cases from them.
