//! Generates a starting expected-values JSON for a fixture FIT file.
//!
//! ```sh
//! cargo run -p zerofit --example gen_fixture_expectations -- tests/fixtures/ride.fit > tests/fixtures/ride.json
//! ```
//!
//! Counts come from zerofit and are cross-checked against the independent
//! `fitparser` crate; any disagreement is reported on stderr and the process
//! exits with status 2. Review the output before committing it, and add
//! `spot_checks` from an independent tool such as Garmin's `FitCSVTool`.

#[path = "../tests/common/summary.rs"]
mod summary;

use std::collections::{BTreeMap, HashSet};
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: gen_fixture_expectations <file.fit>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let summary = match summary::summarize(&bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot summarize: {e}");
            return ExitCode::FAILURE;
        }
    };

    let status = cross_check(&bytes, &summary.by_global_message);

    match serde_json::to_string_pretty(&summary) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("cannot serialize: {e}");
            return ExitCode::FAILURE;
        }
    }
    status
}

/// Compares per-message counts with fitparser's.
fn cross_check(bytes: &[u8], ours: &BTreeMap<u16, usize>) -> ExitCode {
    use fitparser::de::DecodeOption;

    let options: HashSet<_> = [
        DecodeOption::SkipHeaderCrcValidation,
        DecodeOption::SkipDataCrcValidation,
    ]
    .into_iter()
    .collect();
    let records = match fitparser::de::from_bytes_with_options(bytes, &options) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: fitparser failed ({e}); counts are NOT cross-checked");
            return ExitCode::from(2);
        }
    };
    let mut theirs: BTreeMap<u16, usize> = BTreeMap::new();
    for record in &records {
        let count = theirs.entry(record.kind().as_u16()).or_default();
        *count = count.saturating_add(1);
    }
    if &theirs == ours {
        eprintln!(
            "ok: message counts match fitparser ({} messages)",
            records.len()
        );
        ExitCode::SUCCESS
    } else {
        eprintln!("MISMATCH with fitparser:\n  zerofit:   {ours:?}\n  fitparser: {theirs:?}");
        ExitCode::from(2)
    }
}
