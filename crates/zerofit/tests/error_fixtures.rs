//! Error cases generated from every good fixture, with the expected error
//! kind and byte offset computed independently of zerofit.
//!
//! An independent record walker (below) finds the file layout; each case
//! corrupts a copy of the fixture in one specific way and states exactly
//! which error a validating decoder must report and where. Both the slice
//! decoder and the streaming decoder are checked.

#![cfg(feature = "std")]
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_lines
)]

mod common;

use common::builder::crc16;
use common::expected::load_fixtures;
use zerofit::{DecodeOptions, Decoder, Error, ErrorKind};

/// One record found by the walker.
#[derive(Debug, Clone, Copy)]
struct Rec {
    offset: usize,
    len: usize,
    kind: RecKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RecKind {
    /// A definition; `stages` are the absolute offsets at which each part
    /// of it (fixed part, fields, developer count, developer fields) ends.
    Definition {
        local: u8,
        stages: [usize; 4],
    },
    Data {
        local: u8,
    },
}

/// The first file's layout, found without zerofit.
#[derive(Debug)]
struct Layout {
    header_size: usize,
    data_end: usize,
    records: Vec<Rec>,
}

fn walk(bytes: &[u8]) -> Layout {
    let header_size = usize::from(bytes[0]);
    let data_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let data_end = header_size + data_size;
    let mut sizes = [None::<usize>; 16];
    let mut records = Vec::new();
    let mut at = header_size;
    while at < data_end {
        let h = bytes[at];
        let rec = if h & 0x80 == 0 && h & 0x40 != 0 {
            let local = h & 0x0F;
            let n = usize::from(bytes[at + 5]);
            let fields = &bytes[at + 6..at + 6 + 3 * n];
            let fixed_end = at + 6;
            let fields_end = fixed_end + 3 * n;
            let (dev_count_end, dev_end, dev_size) = if h & 0x20 != 0 {
                let m = usize::from(bytes[fields_end]);
                let dev = &bytes[fields_end + 1..fields_end + 1 + 3 * m];
                let size: usize = dev.chunks(3).map(|c| usize::from(c[1])).sum();
                (fields_end + 1, fields_end + 1 + 3 * m, size)
            } else {
                (fields_end, fields_end, 0)
            };
            let size: usize = fields.chunks(3).map(|c| usize::from(c[1])).sum();
            sizes[usize::from(local)] = Some(size + dev_size);
            Rec {
                offset: at,
                len: dev_end - at,
                kind: RecKind::Definition {
                    local,
                    stages: [fixed_end, fields_end, dev_count_end, dev_end],
                },
            }
        } else {
            let local = if h & 0x80 != 0 {
                (h >> 5) & 0x03
            } else {
                h & 0x0F
            };
            let size = sizes[usize::from(local)].expect("fixture uses an undefined local type");
            Rec {
                offset: at,
                len: 1 + size,
                kind: RecKind::Data { local },
            }
        };
        at += rec.len;
        records.push(rec);
    }
    assert_eq!(at, data_end, "records must end exactly at the data size");
    Layout {
        header_size,
        data_end,
        records,
    }
}

fn eof(needed: usize, offset: usize) -> Error {
    Error::new(ErrorKind::UnexpectedEof { needed }, offset as u64)
}

/// Rewrites the file CRC (and the header CRC, if present) of the first file.
fn fix_crcs(bytes: &mut [u8], layout: &Layout) {
    if layout.header_size == 14 {
        let crc = crc16(&bytes[..12]);
        bytes[12..14].copy_from_slice(&crc.to_le_bytes());
    }
    let crc = crc16(&bytes[..layout.data_end]);
    bytes[layout.data_end..layout.data_end + 2].copy_from_slice(&crc.to_le_bytes());
}

/// The first error each decoder reports.
fn first_errors(bytes: &[u8], options: DecodeOptions) -> (Option<Error>, Option<Error>) {
    let slice = Decoder::with_options(bytes, options).find_map(Result::err);
    let mut stream = zerofit::ReadDecoder::with_options(std::io::Cursor::new(bytes), options);
    let streamed = loop {
        match stream.next_record() {
            None => break None,
            Some(Ok(_)) => {}
            Some(Err(zerofit::ReadError::Decode(e))) => break Some(e),
            Some(Err(e)) => panic!("I/O error from a cursor: {e}"),
        }
    };
    (slice, streamed)
}

struct Case {
    name: String,
    bytes: Vec<u8>,
    options: DecodeOptions,
    expected: Error,
    /// Whether the streaming decoder must report the same error. It checks
    /// the file CRC only at the end of the file, so a corruption that also
    /// breaks the structure surfaces there as a structural error first.
    stream_same: bool,
}

fn cases(bytes: &[u8]) -> Vec<Case> {
    let layout = walk(bytes);
    let mut out = Vec::new();
    let mut push = |name: String, bytes: Vec<u8>, options, expected, stream_same| {
        out.push(Case {
            name,
            bytes,
            options,
            expected,
            stream_same,
        });
    };
    macro_rules! case {
        ($($arg:expr),+ $(,)?) => {
            push($($arg),+, true)
        };
    }
    let strict = DecodeOptions::new();
    let end = layout.data_end;
    let first = &bytes[..end + 2];

    // Truncation inside the header.
    case!("empty".into(), Vec::new(), strict, eof(12, 0));
    for cut in [1, 5, layout.header_size - 1] {
        let e = eof(layout.header_size - cut, 0);
        case!(
            format!("truncated header at {cut}"),
            first[..cut].to_vec(),
            strict,
            e,
        );
    }

    // Truncation at and inside records: the decoder reports the start of the
    // record and how many more bytes it needs to make progress.
    let picks = [0, 1, layout.records.len() / 2, layout.records.len() - 1];
    for &i in &picks {
        let r = layout.records[i];
        let mut cuts = vec![(r.offset, eof(1, r.offset))];
        match r.kind {
            RecKind::Data { .. } => {
                let cut = r.offset + r.len / 2 + 1;
                cuts.push((cut, eof(r.offset + r.len - cut, r.offset)));
            }
            RecKind::Definition { stages, .. } => {
                for cut in [r.offset + 1, r.offset + 6, r.offset + r.len - 1] {
                    if cut >= r.offset + r.len {
                        continue;
                    }
                    let stage_end = stages.iter().copied().find(|&s| s > cut).unwrap();
                    cuts.push((cut, eof(stage_end - cut, r.offset)));
                }
            }
        }
        for (cut, e) in cuts {
            case!(
                format!("truncated in record {i} at {cut}"),
                first[..cut].to_vec(),
                strict,
                e,
            );
        }
    }

    // Truncation in the file CRC.
    case!(
        "no file CRC".into(),
        first[..end].to_vec(),
        strict,
        eof(2, end),
    );
    case!(
        "half a file CRC".into(),
        first[..=end].to_vec(),
        strict,
        eof(1, end),
    );

    // A flipped data byte: the slice decoder's up-front CRC check fails
    // before any record is decoded. Flipping the last byte of a data message
    // changes a value, not the structure, so the streaming decoder reaches
    // the end and reports the same error; flipping the first record header
    // breaks the structure first.
    let last_data = layout
        .records
        .iter()
        .rev()
        .find(|r| matches!(r.kind, RecKind::Data { .. }) && r.len > 1)
        .unwrap();
    for (at, stream_same) in [
        (layout.header_size, false),
        (last_data.offset + last_data.len - 1, true),
    ] {
        let mut b = first.to_vec();
        b[at] ^= 0x01;
        let stored = u16::from_le_bytes([b[end], b[end + 1]]);
        let computed = crc16(&b[..end]);
        let e = Error::new(ErrorKind::FileCrcMismatch { stored, computed }, end as u64);
        push(format!("flipped byte {at}"), b, strict, e, stream_same);
    }

    // A flipped header byte (profile version) breaks the header CRC.
    if layout.header_size == 14 && first[12..14] != [0, 0] {
        let mut b = first.to_vec();
        b[2] ^= 0x01;
        let stored = u16::from_le_bytes([b[12], b[13]]);
        let computed = crc16(&b[..12]);
        let e = Error::new(ErrorKind::HeaderCrcMismatch { stored, computed }, 0);
        case!("flipped header byte".into(), b, strict, e);
    }

    // Bad signature and header size.
    let mut b = first.to_vec();
    b[8..12].copy_from_slice(b".FIX");
    case!(
        "bad signature".into(),
        b,
        strict,
        Error::new(ErrorKind::InvalidSignature(*b".FIX"), 0),
    );
    let mut b = first.to_vec();
    b[0] = 13;
    case!(
        "bad header size".into(),
        b,
        strict,
        Error::new(ErrorKind::InvalidHeaderSize(13), 0),
    );

    // A data message using a local type with no definition yet (CRCs fixed
    // so the decoder gets that far).
    let mut defined = [false; 16];
    for r in &layout.records {
        match r.kind {
            RecKind::Definition { local, .. } => defined[usize::from(local)] = true,
            RecKind::Data { .. } => {
                if let Some(free) = (0..16u8).find(|&l| !defined[usize::from(l)]) {
                    let mut b = first.to_vec();
                    b[r.offset] = free; // normal data header, local type `free`
                    fix_crcs(&mut b, &layout);
                    let e = Error::new(ErrorKind::UndefinedLocalMessage(free), r.offset as u64);
                    case!(format!("undefined local {free}"), b, strict, e);
                }
                break;
            }
        }
    }

    // An invalid architecture byte in the first definition.
    let def = layout
        .records
        .iter()
        .find(|r| matches!(r.kind, RecKind::Definition { .. }))
        .unwrap();
    let mut b = first.to_vec();
    b[def.offset + 2] = 7;
    fix_crcs(&mut b, &layout);
    let e = Error::new(ErrorKind::InvalidArchitecture(7), def.offset as u64);
    case!("invalid architecture".into(), b, strict, e);

    // A header whose data size ends one byte before the last record does.
    // (The file CRC no longer lines up, so only structure is validated.)
    let last = layout.records.last().unwrap();
    let mut b = first.to_vec();
    let short = u32::try_from(end - layout.header_size - 1).unwrap();
    b[4..8].copy_from_slice(&short.to_le_bytes());
    if layout.header_size == 14 {
        let crc = crc16(&b[..12]);
        b[12..14].copy_from_slice(&crc.to_le_bytes());
    }
    let e = Error::new(
        ErrorKind::DataSizeOverrun { overrun: 1 },
        last.offset as u64,
    );
    case!(
        "data size overrun".into(),
        b,
        DecodeOptions::new().validate_file_crc(false),
        e,
    );

    out
}

#[test]
fn every_error_case_reports_the_expected_kind_and_offset() {
    let mut failures = Vec::new();
    let mut count = 0;
    for f in load_fixtures() {
        for c in cases(&f.bytes) {
            count += 1;
            let (slice, stream) = first_errors(&c.bytes, c.options);
            if slice != Some(c.expected) {
                failures.push(format!(
                    "{} / {}: slice decoder: expected {:?}, got {slice:?}",
                    f.name, c.name, c.expected
                ));
            }
            if c.stream_same && stream != Some(c.expected) || stream.is_none() {
                failures.push(format!(
                    "{} / {}: streaming decoder: expected {:?}, got {stream:?}",
                    f.name, c.name, c.expected
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{count} error cases");
}

/// The walker agrees with the fixture's FitCSVTool record counts, so the
/// expectations above rest on a correct layout.
#[test]
fn walker_matches_ground_truth() {
    for f in load_fixtures() {
        let layout = walk(&f.bytes);
        let defs = layout
            .records
            .iter()
            .filter(|r| matches!(r.kind, RecKind::Definition { .. }))
            .count();
        assert_eq!(defs, f.expected.definitions, "{}", f.name);
        assert_eq!(
            layout.records.len() - defs,
            f.expected.data_messages,
            "{}",
            f.name
        );
    }
}
