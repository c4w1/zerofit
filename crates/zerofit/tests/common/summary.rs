//! Fixture expectations: the JSON schema, how to compute a summary of a FIT
//! file with zerofit, and how to compare the two.
//!
//! Shared by `tests/fixtures.rs` and `examples/gen_fixture_expectations.rs`.

#![allow(
    dead_code,
    unreachable_pub,
    missing_docs,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::float_cmp
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::string::String;
use std::vec::Vec;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use zerofit::{DecodeOptions, Decoder, Record, Value};

/// Expected (or actual) facts about a fixture. See CLAUDE.md for the format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    /// Header of the first file in the stream.
    pub header: HeaderSummary,
    /// Every 14-byte header's CRC is zero or correct.
    pub header_crc_valid: bool,
    /// Every file's trailing CRC is correct.
    pub file_crc_valid: bool,
    pub files_in_stream: usize,
    pub definitions: usize,
    pub data_messages: usize,
    /// Data message counts keyed by global message number.
    pub by_global_message: BTreeMap<u16, usize>,
    /// Field values to check in specific data messages.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spot_checks: Vec<SpotCheck>,
    /// The error a fully validating decoder must stop with, if any.
    #[serde(default)]
    pub expect_error: Option<ExpectedError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderSummary {
    pub size: u8,
    pub protocol_version: u8,
    pub profile_version: u16,
    pub data_size: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpotCheck {
    /// Zero-based index among all data messages in the stream.
    pub index: usize,
    /// Expected global message number of that message.
    pub global: u16,
    /// Field number to expected value: a number, a string, an array of
    /// numbers, or `null` for "invalid".
    pub fields: BTreeMap<u8, Json>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedError {
    /// `ErrorKind` variant name, for example `"FileCrcMismatch"`.
    pub kind: String,
    pub offset: u64,
}

/// The `ErrorKind` variant name of an error.
pub fn kind_name(e: &zerofit::Error) -> String {
    let debug = format!("{:?}", e.kind());
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .into()
}

/// Summarizes `bytes` with zerofit. `spot_checks` is left empty.
///
/// Counts come from a decoder with CRC validation off, so a file with a bad
/// CRC is still fully counted; `expect_error` comes from a validating one.
pub fn summarize(bytes: &[u8]) -> Result<Summary, String> {
    let expect_error = Decoder::new(bytes)
        .find_map(Result::err)
        .map(|e| ExpectedError {
            kind: kind_name(&e),
            offset: e.offset(),
        });

    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);
    let mut decoder = Decoder::with_options(bytes, options);
    let mut first_header = None;
    let mut s = Summary {
        header: HeaderSummary {
            size: 0,
            protocol_version: 0,
            profile_version: 0,
            data_size: 0,
        },
        header_crc_valid: true,
        file_crc_valid: true,
        files_in_stream: 0,
        definitions: 0,
        data_messages: 0,
        by_global_message: BTreeMap::new(),
        spot_checks: Vec::new(),
        expect_error,
    };
    let mut file_start = 0usize;
    loop {
        let before = usize::try_from(decoder.position()).map_err(|e| e.to_string())?;
        let Some(record) = decoder.next() else { break };
        let Ok(record) = record else { break };
        match record {
            Record::Header(h) => {
                file_start = before;
                s.files_in_stream += 1;
                first_header.get_or_insert(HeaderSummary {
                    size: h.size(),
                    protocol_version: h.protocol_version().to_byte(),
                    profile_version: h.profile_version(),
                    data_size: h.data_size(),
                });
                if let Some(stored) = h.crc().filter(|&c| c != 0) {
                    let computed = zerofit::crc::crc16(&bytes[before..before + 12]);
                    s.header_crc_valid &= stored == computed;
                }
            }
            Record::Definition(_) => s.definitions += 1,
            Record::Data(m) => {
                s.data_messages += 1;
                *s.by_global_message
                    .entry(m.global_message_number())
                    .or_default() += 1;
            }
            Record::FileEnd { crc } => {
                s.file_crc_valid &= crc == zerofit::crc::crc16(&bytes[file_start..before]);
            }
        }
    }
    s.header = first_header.ok_or("no FIT header could be decoded")?;
    Ok(s)
}

/// Compares an actual summary against expectations (ignoring spot checks) and
/// returns human-readable differences.
pub fn diff(expected: &Summary, actual: &Summary) -> Vec<String> {
    let mut out = Vec::new();
    macro_rules! cmp {
        ($field:ident) => {
            if expected.$field != actual.$field {
                out.push(format!(
                    "{}: expected {:?}, got {:?}",
                    stringify!($field),
                    expected.$field,
                    actual.$field
                ));
            }
        };
    }
    cmp!(header);
    cmp!(header_crc_valid);
    cmp!(file_crc_valid);
    cmp!(files_in_stream);
    cmp!(definitions);
    cmp!(data_messages);
    cmp!(by_global_message);
    cmp!(expect_error);
    out
}

/// Evaluates spot checks against the data messages in `bytes`.
pub fn check_spots(bytes: &[u8], spots: &[SpotCheck]) -> Vec<String> {
    let mut out = Vec::new();
    if spots.is_empty() {
        return out;
    }
    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);
    let messages: Vec<_> = Decoder::with_options(bytes, options)
        .map_while(Result::ok)
        .filter_map(|r| match r {
            Record::Data(m) => Some(m),
            _ => None,
        })
        .collect();
    for spot in spots {
        let Some(msg) = messages.get(spot.index) else {
            out.push(format!(
                "spot check {}: only {} data messages",
                spot.index,
                messages.len()
            ));
            continue;
        };
        if msg.global_message_number() != spot.global {
            out.push(format!(
                "spot check {}: expected global {}, got {}",
                spot.index,
                spot.global,
                msg.global_message_number()
            ));
            continue;
        }
        for (&number, want) in &spot.fields {
            let Some(field) = msg.field(number) else {
                out.push(format!("spot check {}: field {number} missing", spot.index));
                continue;
            };
            if let Err(why) = value_matches(field.value(), want) {
                let mut line = String::new();
                let _ = write!(
                    line,
                    "spot check {} field {number}: {why} (raw {:?})",
                    spot.index,
                    field.raw_value()
                );
                out.push(line);
            }
        }
    }
    out
}

fn value_matches(got: Option<Value<'_>>, want: &Json) -> Result<(), String> {
    match (got, want) {
        (None, Json::Null) => Ok(()),
        (None, _) => Err(format!("expected {want}, got invalid")),
        (Some(v), Json::Null) => Err(format!("expected invalid, got {v:?}")),
        (Some(Value::String(s)), Json::String(w)) => match s.to_str() {
            Ok(text) if text == w => Ok(()),
            other => Err(format!("expected {w:?}, got {other:?}")),
        },
        (Some(Value::Array(a)), Json::Array(w)) => {
            if a.len() != w.len() {
                return Err(format!("expected {} elements, got {}", w.len(), a.len()));
            }
            for (i, (elem, want)) in a.iter().zip(w).enumerate() {
                let elem = Some(elem).filter(|e| !e.is_invalid());
                value_matches(elem, want).map_err(|e| format!("element {i}: {e}"))?;
            }
            Ok(())
        }
        (Some(v), Json::Number(n)) => number_matches(&v, n),
        (Some(v), _) => Err(format!("expected {want}, got {v:?}")),
    }
}

fn number_matches(v: &Value<'_>, want: &serde_json::Number) -> Result<(), String> {
    let ok = if let (Some(a), Some(b)) = (v.as_i64(), want.as_i64()) {
        a == b
    } else if let (Value::UInt64(a) | Value::UInt64z(a), Some(b)) = (v, want.as_u64()) {
        *a == b
    } else if let (Some(a), Some(b)) = (v.as_f64(), want.as_f64()) {
        a == b || (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
    } else {
        false
    };
    if ok {
        Ok(())
    } else {
        Err(format!("expected {want}, got {v:?}"))
    }
}
