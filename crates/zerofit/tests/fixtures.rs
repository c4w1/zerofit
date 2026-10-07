//! Integration tests over real FIT files in `tests/fixtures/`.
//!
//! Each `<name>.fit` has a `<name>.expected.json` written by
//! `cargo xtask expected` from Garmin's FitCSVTool, never from zerofit. This
//! file checks the raw layer against it; `zerofit-profile/tests/fixtures.rs`
//! checks the profile layer against the same files.

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

use common::builder::FitBuilder;
use common::expected::{Expected, check_raw, check_spots, load_fixtures, profile_message_numbers};
use zerofit::{Decoder, ErrorKind, Record};

#[test]
fn fixtures_match_fitcsvtool() {
    let fixtures = load_fixtures();
    if fixtures.is_empty() {
        return; // e.g. in the published package, which excludes them
    }
    let named = profile_message_numbers();
    let mut failures = Vec::new();
    for f in fixtures {
        failures.extend(
            check_raw(&f.bytes, &f.expected, &named)
                .into_iter()
                .map(|e| format!("{}: {e}", f.name)),
        );
    }
    assert!(
        failures.is_empty(),
        "
{}",
        failures.join(
            "
"
        )
    );
}

/// The streaming decoder produces exactly the slice decoder's records.
#[cfg(feature = "std")]
#[test]
fn streaming_decoder_matches_slice_decoder_on_fixtures() {
    for f in load_fixtures() {
        let slice: Vec<String> = Decoder::new(&f.bytes)
            .map(|r| format!("{:?}", r.unwrap()))
            .collect();
        let mut stream = Vec::new();
        let mut d = zerofit::ReadDecoder::new(std::io::Cursor::new(&f.bytes));
        while let Some(r) = d.next_record() {
            stream.push(format!("{:?}", r.unwrap()));
        }
        assert_eq!(stream.len(), slice.len(), "{}", f.name);
        assert!(stream == slice, "{}: streams differ", f.name);
    }
}

/// Re-encoding a fixture with the encoder reproduces it byte for byte.
#[cfg(feature = "alloc")]
#[test]
fn encoder_round_trips_fixtures() {
    for f in load_fixtures() {
        let mut enc = zerofit::encode::Encoder::new();
        for r in Decoder::new(&f.bytes) {
            enc.write_record(&r.unwrap()).unwrap();
        }
        assert!(enc.finish().unwrap() == f.bytes, "{}", f.name);
    }
}

/// Corrupting a fixture must produce a precise error, never a panic.
#[test]
fn corrupted_fixtures_fail_cleanly() {
    for f in load_fixtures() {
        let name = &f.name;
        let bytes = &f.bytes;
        let e = &f.expected;
        let file_end = usize::from(e.header.size) + e.header.data_size as usize;

        // Flip one bit in the middle of the first file's data section.
        if e.header.data_size > 0 {
            let mut corrupt = bytes.clone();
            let at = usize::from(e.header.size) + e.header.data_size as usize / 2;
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
        .data(1, &[0, 0, 0, 100, 150, 0x00, 0x01, 0xFF, 0xFF])
        .definition(2, false, 0xFF00, &[(0, 1, 0x02)])
        .data(2, &[1]);
    let bytes = b.build();
    let named = [0u16, 20].into_iter().collect();

    let json = serde_json::json!({
        "generated_by": "test",
        "header": {"size": 14, "protocol_version": 32, "profile_version": 2132,
                   "data_size": b.records_len()},
        "header_crc_valid": true,
        "file_crc_valid": true,
        "files_in_stream": 1,
        "definitions": 3,
        "data_messages": 3,
        "by_global_message": {"0": 1, "20": 1},
        "unknown_messages": 1,
        "spot_checks": [
            {"global": 0, "message": "file_id", "occurrence": 0, "fields": {"0": 4, "8": "Edge"}},
            {"global": 20, "message": "record", "occurrence": 0,
             "fields": {"253": 100, "3": 150, "2": [1, null]}}
        ],
        "profile": {"session": {}}
    });
    let expected: Expected = serde_json::from_value(json).unwrap();
    assert_eq!(check_raw(&bytes, &expected, &named), Vec::<String>::new());

    // Every kind of mismatch is reported.
    let mut wrong = expected.clone();
    wrong.data_messages = 4;
    wrong.unknown_messages = 0;
    wrong.by_global_message.insert(20, 5);
    wrong.spot_checks[0].fields.insert(8, "Edgy".into());
    wrong.spot_checks[1].fields.insert(3, 151.into());
    wrong.spot_checks[1].fields.insert(9, 1.into());
    wrong.spot_checks[1].occurrence = 0;
    let failures = check_raw(&bytes, &wrong, &named);
    assert_eq!(failures.len(), 6, "{failures:#?}");
    wrong.spot_checks[1].occurrence = 1;
    assert_eq!(check_spots(&bytes, &wrong.spot_checks[1..]).len(), 1);

    // A corrupt CRC is reported.
    let bad = b.clone().file_crc(0).build();
    let failures = check_raw(&bad, &expected, &named);
    assert!(
        failures.iter().any(|f| f.contains("file_crc_valid")),
        "{failures:#?}"
    );
    assert!(
        failures.iter().any(|f| f.contains("FileCrcMismatch")),
        "{failures:#?}"
    );
}

/// Chained files are decoded as one stream and each file ends with its CRC.
#[test]
fn chained_files() {
    let mut a = FitBuilder::new();
    a.definition(0, false, 20, &[(3, 1, 2)])
        .data(0, &[1])
        .data(0, &[2]);
    let mut bytes = a.build();
    bytes.extend(a.build());
    let records: Vec<_> = Decoder::new(&bytes).map(Result::unwrap).collect();
    let count = |f: fn(&Record<'_>) -> bool| records.iter().filter(|r| f(r)).count();
    assert_eq!(count(|r| matches!(r, Record::Header(_))), 2);
    assert_eq!(count(|r| matches!(r, Record::Data(_))), 4);
    assert_eq!(count(|r| matches!(r, Record::FileEnd { .. })), 2);
}
