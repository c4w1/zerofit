//! Fixture ground truth (`<name>.expected.json`, written by
//! `cargo xtask expected` from Garmin's FitCSVTool) and the raw-layer checks
//! against it.
//!
//! Shared by `zerofit/tests/fixtures.rs` and `zerofit-profile/tests/fixtures.rs`
//! (included with `#[path]`), so it depends only on `zerofit`, `serde` and
//! `serde_json`.

#![allow(
    dead_code,
    unreachable_pub,
    missing_docs,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::unwrap_used,
    clippy::panic
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value as Json;
use zerofit::{DataMessage, DecodeOptions, Decoder, Record, Value};

/// Ground truth for one fixture. See `xtask/src/expected.rs`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    pub generated_by: String,
    pub header: Header,
    pub header_crc_valid: bool,
    pub file_crc_valid: bool,
    pub files_in_stream: usize,
    pub definitions: usize,
    pub data_messages: usize,
    /// Counts of messages the profile names, by global message number.
    pub by_global_message: BTreeMap<u16, usize>,
    /// Messages FitCSVTool reported as `unknown`.
    pub unknown_messages: usize,
    pub spot_checks: Vec<SpotCheck>,
    pub profile: ProfileValues,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub size: u8,
    pub protocol_version: u8,
    pub profile_version: u16,
    pub data_size: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpotCheck {
    pub global: u16,
    pub message: String,
    /// Zero-based index among messages with this global number.
    pub occurrence: usize,
    /// Field number to raw value: a number, a string or an array.
    pub fields: BTreeMap<u8, Json>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileValues {
    /// Scaled values of the first `session` message, by profile field name.
    pub session: BTreeMap<String, Tolerant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tolerant {
    pub value: f64,
    pub tolerance: f64,
}

/// One fixture: its name, bytes and ground truth.
pub struct Fixture {
    pub name: String,
    pub bytes: Vec<u8>,
    pub expected: Expected,
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zerofit/tests/fixtures")
}

/// Every `.fit` fixture with its expected JSON. Panics if a fixture has no
/// (or an unreadable) expected JSON, so a fixture cannot go unchecked.
pub fn load_fixtures() -> Vec<Fixture> {
    let dir = fixtures_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
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
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let json_path = path.with_extension("expected.json");
            let json = std::fs::read_to_string(&json_path).unwrap_or_else(|e| {
                panic!(
                    "{name}: cannot read {} ({e}); run `cargo xtask expected`",
                    json_path.display()
                )
            });
            let expected = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("{name}: bad expected JSON: {e}"));
            Fixture {
                name,
                bytes: std::fs::read(&path).unwrap(),
                expected,
            }
        })
        .collect()
}

/// Global message numbers the FIT profile names (from the profile subset
/// the code generator uses). `mfg_range_min`/`mfg_range_max` are range
/// markers for manufacturer-specific messages, not messages.
pub fn profile_message_numbers() -> BTreeSet<u16> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../zerofit-profile/codegen/profile-subset.json");
    let json: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    json["mesg_num"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| !v["name"].as_str().unwrap().starts_with("mfg_range"))
        .map(|v| u16::try_from(v["value"].as_u64().unwrap()).unwrap())
        .collect()
}

/// Every data message, decoding with CRC checks off (CRC validity is
/// checked separately).
pub fn data_messages(bytes: &[u8]) -> Vec<DataMessage<'_>> {
    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);
    Decoder::with_options(bytes, options)
        .map(|r| r.unwrap())
        .filter_map(|r| match r {
            Record::Data(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// The `occurrence`-th message with global number `global`.
pub fn nth_message<'a>(
    messages: &[DataMessage<'a>],
    global: u16,
    occurrence: usize,
) -> Option<DataMessage<'a>> {
    messages
        .iter()
        .filter(|m| m.global_message_number() == global)
        .nth(occurrence)
        .copied()
}

/// Checks everything the raw layer can see. Returns human-readable failures.
pub fn check_raw(bytes: &[u8], expected: &Expected, named: &BTreeSet<u16>) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |what: &str, want: &dyn std::fmt::Debug, got: &dyn std::fmt::Debug| {
        let (w, g) = (format!("{want:?}"), format!("{got:?}"));
        if w != g {
            out.push(format!("{what}: expected {w}, got {g}"));
        }
    };

    // A fully validating decode must succeed iff FitCSVTool accepted the file.
    let first_error = Decoder::new(bytes).find_map(Result::err);
    push(
        "validating decode error",
        &None::<zerofit::Error>,
        &first_error,
    );

    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);
    let mut header = None;
    let mut files = 0;
    let mut definitions = 0;
    let mut by_global = BTreeMap::<u16, usize>::new();
    let mut unknown = 0;
    let mut header_crc_valid = true;
    let mut file_crc_valid = true;
    let mut file_start = 0usize;
    let mut decoder = Decoder::with_options(bytes, options);
    loop {
        let before = usize::try_from(decoder.position()).unwrap();
        let Some(record) = decoder.next() else { break };
        match record.unwrap() {
            Record::Header(h) => {
                file_start = before;
                files += 1;
                header.get_or_insert(Header {
                    size: h.size(),
                    protocol_version: h.protocol_version().to_byte(),
                    profile_version: h.profile_version(),
                    data_size: h.data_size(),
                });
                if let Some(stored) = h.crc().filter(|&c| c != 0) {
                    header_crc_valid &= stored == zerofit::crc::crc16(&bytes[before..before + 12]);
                }
            }
            Record::Definition(_) => definitions += 1,
            Record::Data(m) => {
                let global = m.global_message_number();
                if named.contains(&global) {
                    *by_global.entry(global).or_default() += 1;
                } else {
                    unknown += 1;
                }
            }
            Record::FileEnd { crc } => {
                file_crc_valid &= crc == zerofit::crc::crc16(&bytes[file_start..before]);
            }
        }
    }
    push("header", &Some(expected.header), &header);
    push(
        "header_crc_valid",
        &expected.header_crc_valid,
        &header_crc_valid,
    );
    push("file_crc_valid", &expected.file_crc_valid, &file_crc_valid);
    push("files_in_stream", &expected.files_in_stream, &files);
    push("definitions", &expected.definitions, &definitions);
    let data: usize = by_global.values().sum::<usize>() + unknown;
    push("data_messages", &expected.data_messages, &data);
    push("by_global_message", &expected.by_global_message, &by_global);
    push("unknown_messages", &expected.unknown_messages, &unknown);
    out.extend(check_spots(bytes, &expected.spot_checks));
    out
}

/// Evaluates spot checks against the data messages in `bytes`.
pub fn check_spots(bytes: &[u8], spots: &[SpotCheck]) -> Vec<String> {
    let messages = data_messages(bytes);
    let mut out = Vec::new();
    for spot in spots {
        let label = format!("{} #{}", spot.message, spot.occurrence);
        let Some(msg) = nth_message(&messages, spot.global, spot.occurrence) else {
            out.push(format!("{label}: message missing"));
            continue;
        };
        for (&number, want) in &spot.fields {
            let Some(field) = msg.field(number) else {
                out.push(format!("{label} field {number}: missing"));
                continue;
            };
            if let Err(why) = value_matches(field.value(), want) {
                out.push(format!(
                    "{label} field {number}: {why} (raw {:?})",
                    field.raw_value()
                ));
            }
        }
    }
    out
}

pub fn value_matches(got: Option<Value<'_>>, want: &Json) -> Result<(), String> {
    match (got, want) {
        (None, Json::Null) => Ok(()),
        (None, _) => Err(format!("expected {want}, got invalid")),
        (Some(v), Json::Null) => Err(format!("expected invalid, got {v:?}")),
        (Some(Value::String(s)), Json::String(w)) => match s.to_str() {
            Ok(text) if text == w => Ok(()),
            other => Err(format!("expected {w:?}, got {other:?}")),
        },
        (Some(Value::Bytes(b)), Json::Array(w)) => {
            let got: Vec<Json> = b.iter().map(|&x| Json::from(x)).collect();
            if &got == w {
                Ok(())
            } else {
                Err(format!("expected {want}, got {got:?}"))
            }
        }
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
        // FitCSVTool prints float32 values via Java's float-to-string, so
        // allow single-precision rounding.
        a == b || (a - b).abs() <= 1e-6 * a.abs().max(b.abs())
    } else {
        false
    };
    if ok {
        Ok(())
    } else {
        Err(format!("expected {want}, got {v:?}"))
    }
}
