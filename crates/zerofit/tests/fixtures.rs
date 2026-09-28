//! Integration tests over real FIT files in `tests/fixtures/`.
//!
//! Each `<name>.fit` must have a `<name>.json` next to it with the expected
//! values (schema in `common/summary.rs` and CLAUDE.md). Generate a starting
//! point with
//!
//! ```sh
//! cargo run -p zerofit --example gen_fixture_expectations -- tests/fixtures/<name>.fit
//! ```
//!
//! then review it and add spot checks from an independent tool.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::builder::FitBuilder;
use common::summary::{Summary, check_spots, diff, summarize};
use zerofit::{Decoder, ErrorKind, Record};

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fit")))
        .collect();
    paths.sort();
    if paths.is_empty() {
        eprintln!("no .fit fixtures in {}; skipping", dir.display());
    }
    paths
}

fn check(bytes: &[u8], expected: &Summary) -> Vec<String> {
    let mut failures = match summarize(bytes) {
        Ok(actual) => diff(expected, &actual),
        Err(e) => vec![e],
    };
    failures.extend(check_spots(bytes, &expected.spot_checks));
    failures
}

#[test]
fn fixtures_match_expectations() {
    let mut failures = Vec::new();
    for path in fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let json_path = path.with_extension("json");
        let Ok(json) = fs::read_to_string(&json_path) else {
            failures.push(format!(
                "{name}: missing {} (generate it with the gen_fixture_expectations example)",
                json_path.display()
            ));
            continue;
        };
        let expected: Summary = match serde_json::from_str(&json) {
            Ok(e) => e,
            Err(e) => {
                failures.push(format!("{name}: bad JSON: {e}"));
                continue;
            }
        };
        let bytes = fs::read(&path).unwrap();
        failures.extend(
            check(&bytes, &expected)
                .into_iter()
                .map(|f| format!("{name}: {f}")),
        );
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Corrupting a fixture must produce a precise error, never a panic.
#[test]
fn corrupted_fixtures_fail_cleanly() {
    for path in fixtures() {
        let name = path.display();
        let bytes = fs::read(&path).unwrap();
        let Ok(summary) = summarize(&bytes) else {
            continue;
        };
        let file_end = usize::from(summary.header.size) + summary.header.data_size as usize;
        if summary.expect_error.is_some() || !summary.file_crc_valid || file_end + 2 > bytes.len() {
            continue; // Only corrupt fixtures that are valid to begin with.
        }

        // Flip one bit in the middle of the first file's data section.
        if summary.header.data_size > 0 {
            let mut corrupt = bytes.clone();
            let at = usize::from(summary.header.size) + summary.header.data_size as usize / 2;
            corrupt[at] ^= 0x10;
            let first = Decoder::new(&corrupt).next().unwrap().unwrap_err();
            assert!(
                matches!(first.kind(), ErrorKind::FileCrcMismatch { .. }),
                "{name}: {first:?}"
            );
            assert_eq!(first.offset(), file_end as u64, "{name}");
        }

        // Truncate at evenly spaced points inside the first file.
        let step = (file_end / 64).max(1);
        for len in (0..file_end + 2).step_by(step) {
            let items: Vec<_> = Decoder::new(&bytes[..len]).collect();
            let last = items.last().unwrap();
            assert!(
                matches!(last, Err(e) if matches!(e.kind(), ErrorKind::UnexpectedEof { .. })),
                "{name} truncated to {len}: {last:?}"
            );
        }
    }
}

/// The harness itself must detect mismatches, so a broken harness cannot
/// silently pass real fixtures.
#[test]
fn harness_self_test() {
    let mut b = FitBuilder::new();
    b.definition(0, false, 0, &[(0, 1, 0x00), (8, 6, 0x07)])
        .data(0, &[4, b'E', b'd', b'g', b'e', 0, 0])
        .definition(1, true, 20, &[(253, 4, 0x86), (3, 1, 0x02), (2, 4, 0x84)])
        .data(1, &[0, 0, 0, 100, 150, 0x00, 0x01, 0xFF, 0xFF]);
    let bytes = b.build();

    let mut expected = summarize(&bytes).unwrap();
    assert_eq!(expected.files_in_stream, 1);
    assert_eq!(expected.data_messages, 2);
    assert_eq!(expected.definitions, 2);
    assert!(expected.header_crc_valid && expected.file_crc_valid);
    assert_eq!(expected.expect_error, None);

    expected.spot_checks = serde_json::from_str(
        r#"[
            {"index": 0, "global": 0, "fields": {"0": 4, "8": "Edge"}},
            {"index": 1, "global": 20, "fields": {"253": 100, "3": 150, "2": [1, null]}}
        ]"#,
    )
    .unwrap();
    assert_eq!(check(&bytes, &expected), Vec::<String>::new());

    // Every kind of mismatch is reported.
    let mut wrong = expected.clone();
    wrong.data_messages = 3;
    wrong.by_global_message.insert(20, 5);
    wrong.spot_checks[0].fields.insert(8, "Edgy".into());
    wrong.spot_checks[1].fields.insert(3, 151.into());
    wrong.spot_checks[1].fields.insert(9, 1.into());
    let failures = check(&bytes, &wrong);
    assert_eq!(failures.len(), 5, "{failures:#?}");

    // A corrupt CRC shows up in the summary and as the expected error.
    let bad = b.clone().file_crc(0).build();
    let s = summarize(&bad).unwrap();
    assert!(!s.file_crc_valid);
    assert_eq!(s.expect_error.unwrap().kind, "FileCrcMismatch");
    assert_eq!(s.data_messages, 2);
}

/// Sanity check that the decoder handles files it has not seen in the
/// fixture set: chained files are summarized as one stream.
#[test]
fn harness_counts_chained_files() {
    let mut a = FitBuilder::new();
    a.definition(0, false, 20, &[(3, 1, 2)])
        .data(0, &[1])
        .data(0, &[2]);
    let mut bytes = a.build();
    bytes.extend(a.build());
    let s = summarize(&bytes).unwrap();
    assert_eq!(s.files_in_stream, 2);
    assert_eq!(s.by_global_message.get(&20), Some(&4));
    let ends = Decoder::new(&bytes)
        .filter(|r| matches!(r, Ok(Record::FileEnd { .. })))
        .count();
    assert_eq!(ends, 2);
}
