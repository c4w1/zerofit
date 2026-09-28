# Fixtures

Real FIT files (`<name>.fit`) with expected values (`<name>.json`), checked by
`tests/fixtures.rs`. The schema is documented in the repository's CLAUDE.md.

Rules:

- Only your own recordings. Never Garmin FIT SDK sample files.
- Check for location data you would rather not publish (home, work).

Generate a starting JSON, review it, then add `spot_checks`:

```sh
cargo run -p zerofit --example gen_fixture_expectations -- crates/zerofit/tests/fixtures/<name>.fit > crates/zerofit/tests/fixtures/<name>.json
```
