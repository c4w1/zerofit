//! `cargo xtask expected`: ground truth for fixtures from Garmin's FitCSVTool.
//!
//! For every `crates/zerofit/tests/fixtures/<name>.fit`, runs
//! `java -jar $FIT_CSV_TOOL -i -re -b <name>.fit <tmp>.csv` and writes
//! `<name>.expected.json`. Nothing here uses zerofit: message counts, field
//! values and session values come from FitCSVTool's CSV, and the header and
//! CRCs are checked by the small independent implementation below. Names in
//! the CSV are mapped to numbers, and scaled values back to raw values, with
//! the FIT profile subset (`profile-subset.json`, extracted from the SDK).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::{Value as Json, json};
use zerofit_codegen::{Field, Profile};

/// Fields with several components are scaled per component; their raw
/// value cannot be recovered from FitCSVTool's output.
fn has_multi_scale(scale: &[f64]) -> bool {
    scale.len() > 1
}

#[derive(Debug, Serialize)]
struct Expected {
    generated_by: String,
    header: Header,
    header_crc_valid: bool,
    file_crc_valid: bool,
    files_in_stream: usize,
    definitions: usize,
    data_messages: usize,
    /// Counts by global message number, for messages the profile names.
    by_global_message: BTreeMap<u16, usize>,
    /// Messages FitCSVTool reports as `unknown` (not in the profile).
    unknown_messages: usize,
    spot_checks: Vec<SpotCheck>,
    profile: ProfileValues,
}

#[derive(Debug, Serialize)]
struct Header {
    size: u8,
    protocol_version: u8,
    profile_version: u16,
    data_size: u32,
}

#[derive(Debug, Serialize)]
struct SpotCheck {
    global: u16,
    message: String,
    /// Zero-based index among the messages with this global number.
    occurrence: usize,
    /// Field number to raw value: integer, float, string or array.
    fields: BTreeMap<u8, Json>,
}

#[derive(Debug, Serialize)]
struct ProfileValues {
    /// Scaled values of the first `session` message, by field name.
    session: BTreeMap<String, Tolerant>,
}

#[derive(Debug, Serialize)]
struct Tolerant {
    value: f64,
    tolerance: f64,
}

/// Messages spot-checked first and last: those with a typed profile view.
const SPOT_MESSAGES: &[&str] = &[
    "file_id",
    "record",
    "lap",
    "session",
    "event",
    "device_info",
    "activity",
    "hrv",
];

pub(crate) fn run(fixtures: &Path, profile: &Profile) -> Result<(), String> {
    let tool = std::env::var_os("FIT_CSV_TOOL")
        .map(PathBuf::from)
        .ok_or("set FIT_CSV_TOOL to the path of FitCSVTool.jar")?;
    let mut paths: Vec<_> = std::fs::read_dir(fixtures)
        .map_err(|e| format!("{}: {e}", fixtures.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("fit")))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no .fit files in {}", fixtures.display()));
    }
    for path in paths {
        let expected =
            expected_for(&path, &tool, profile).map_err(|e| format!("{}: {e}", path.display()))?;
        let out = path.with_extension("expected.json");
        let json = serde_json::to_string_pretty(&expected).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&out, json).map_err(|e| e.to_string())?;
        eprintln!(
            "wrote {} ({} messages)",
            out.display(),
            expected.data_messages
        );
    }
    Ok(())
}

fn expected_for(path: &Path, tool: &Path, profile: &Profile) -> Result<Expected, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let files = independent::files(&bytes)?;
    let first = files.first().ok_or("no FIT header")?;
    let header_crc_valid = files.iter().all(|f| f.header_crc_ok);
    let file_crc_valid = files.iter().all(|f| f.crc_ok);

    let rows = run_tool(tool, path)?;
    if !(header_crc_valid && file_crc_valid) {
        return Err("fixture has a bad CRC; fixtures must be valid files".into());
    }

    let mesg_num: BTreeMap<&str, u16> = profile
        .mesg_num
        .iter()
        .filter_map(|v| Some((v.name.as_str(), u16::try_from(v.value).ok()?)))
        .collect();

    let mut definitions = 0;
    let mut data = Vec::new();
    for row in &rows {
        match row.first().map(String::as_str) {
            Some("Definition") => definitions += 1,
            Some("Data") => data.push(row),
            _ => {}
        }
    }

    let mut by_global_message = BTreeMap::new();
    let mut unknown_messages = 0;
    let mut by_name: BTreeMap<&str, Vec<&Vec<String>>> = BTreeMap::new();
    for row in &data {
        let name = row.get(2).map_or("", String::as_str);
        match mesg_num.get(name) {
            Some(&n) => *by_global_message.entry(n).or_insert(0) += 1,
            None => unknown_messages += 1,
        }
        by_name.entry(name).or_default().push(row);
    }

    let mut spot_checks = Vec::new();
    for message in SPOT_MESSAGES {
        let Some(rows) = by_name.get(message) else {
            continue;
        };
        let global = mesg_num[message];
        let fields = &profile
            .messages
            .iter()
            .find(|m| m.name == *message)
            .ok_or("message missing from profile subset")?
            .fields;
        let mut occurrences = vec![0];
        if rows.len() > 1 {
            occurrences.push(rows.len() - 1);
        }
        for occurrence in occurrences {
            spot_checks.push(SpotCheck {
                global,
                message: (*message).to_owned(),
                occurrence,
                fields: raw_fields(rows[occurrence], fields)?,
            });
        }
    }

    let session = by_name
        .get("session")
        .and_then(|rows| rows.first())
        .map(|row| session_values(row, profile))
        .transpose()?
        .unwrap_or_default();

    Ok(Expected {
        generated_by: format!(
            "cargo xtask expected: FitCSVTool -i -re (FIT SDK profile {})",
            profile.sdk_version
        ),
        header: Header {
            size: first.size,
            protocol_version: first.protocol_version,
            profile_version: first.profile_version,
            data_size: first.data_size,
        },
        header_crc_valid,
        file_crc_valid,
        files_in_stream: files.len(),
        definitions,
        data_messages: data.len(),
        by_global_message,
        unknown_messages,
        spot_checks,
        profile: ProfileValues { session },
    })
}

/// Runs FitCSVTool and returns its CSV rows.
fn run_tool(tool: &Path, fit: &Path) -> Result<Vec<Vec<String>>, String> {
    let dir = std::env::temp_dir().join(format!("zerofit-xtask-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let csv = dir.join("out.csv");
    let _ = std::fs::remove_file(&csv);
    let output = Command::new("java")
        .arg("-jar")
        .arg(tool)
        .args(["-i", "-re", "-b"])
        .arg(fit)
        .arg(&csv)
        .output()
        .map_err(|e| format!("cannot run java: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // FitCSVTool exits 0 even when decoding fails; it reports on stdout.
    if !output.status.success() || stdout.contains("Error:") {
        return Err(format!("FitCSVTool failed:\n{stdout}"));
    }
    let text = std::fs::read_to_string(&csv).map_err(|e| format!("no CSV produced: {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(parse_csv(text.trim_start_matches('\u{feff}')))
}

/// Minimal CSV parser: comma separated, double-quoted fields with `""`
/// escapes. Enough for FitCSVTool's output.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (c, _) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// The (name, value) pairs of a Data row.
fn pairs(row: &[String]) -> impl Iterator<Item = (&str, &str)> {
    row.get(3..)
        .unwrap_or_default()
        .chunks(3)
        .filter_map(|c| Some((c.first()?.as_str(), c.get(1)?.as_str())))
        .filter(|(name, _)| !name.is_empty())
}

/// Maps a CSV field name to (field number, scale, offset, is string).
fn lookup<'f>(fields: &'f [Field], name: &str) -> Option<(u8, &'f [f64], &'f [f64], bool)> {
    for f in fields {
        if f.name == name {
            return Some((f.number, &f.scale, &f.offset, f.field_type == "string"));
        }
        for s in &f.subfields {
            if s.name == name {
                return Some((f.number, &s.scale, &s.offset, s.field_type == "string"));
            }
        }
    }
    None
}

fn raw_fields(row: &[String], fields: &[Field]) -> Result<BTreeMap<u8, Json>, String> {
    let mut out = BTreeMap::new();
    for (name, value) in pairs(row) {
        // Developer fields and fields the profile does not name are skipped.
        let Some((number, scale, offset, is_string)) = lookup(fields, name) else {
            continue;
        };
        if has_multi_scale(scale) || out.contains_key(&number) {
            continue;
        }
        let scale = scale.first().copied().unwrap_or(1.0);
        let offset = offset.first().copied().unwrap_or(0.0);
        let json = if is_string {
            Json::String(value.to_owned())
        } else if value.contains('|') {
            let elems: Result<Vec<_>, _> = value
                .split('|')
                .map(|v| unscale(v, scale, offset))
                .collect();
            Json::Array(elems?)
        } else {
            unscale(value, scale, offset)?
        };
        out.insert(number, json);
    }
    Ok(out)
}

/// FitCSVTool prints `raw / scale - offset`; recover the raw integer.
#[allow(clippy::float_cmp)]
fn unscale(text: &str, scale: f64, offset: f64) -> Result<Json, String> {
    let v: f64 = text
        .trim()
        .parse()
        .map_err(|e| format!("bad number {text:?}: {e}"))?;
    let raw = (v + offset) * scale;
    let rounded = raw.round();
    if (raw - rounded).abs() < 1e-6 && rounded.abs() < 9.0e15 {
        Ok(json!(rounded as i64))
    } else if scale == 1.0 && offset == 0.0 {
        Ok(json!(v)) // a float field
    } else {
        Err(format!("{text} does not unscale to an integer"))
    }
}

fn session_values(row: &[String], profile: &Profile) -> Result<BTreeMap<String, Tolerant>, String> {
    let session = profile
        .messages
        .iter()
        .find(|m| m.name == "session")
        .ok_or("no session in profile subset")?;
    let mut out = BTreeMap::new();
    for (name, value) in pairs(row) {
        let Some(field) = session.fields.iter().find(|f| f.name == name) else {
            continue; // subfields, developer fields
        };
        if field.array.is_some() || field.field_type == "string" || field.field_type == "byte" {
            continue;
        }
        if has_multi_scale(&field.scale) {
            continue;
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            continue;
        };
        let scale = field.scale.first().copied().unwrap_or(1.0);
        out.insert(
            name.to_owned(),
            Tolerant {
                value: v,
                // Half a raw step: any correct decoding rounds to the same raw value.
                tolerance: 0.5 / scale,
            },
        );
    }
    Ok(out)
}

/// An implementation of the header and CRC checks that shares no code with
/// zerofit, so the expected values do not depend on the code under test.
mod independent {
    pub(crate) struct File {
        pub(crate) size: u8,
        pub(crate) protocol_version: u8,
        pub(crate) profile_version: u16,
        pub(crate) data_size: u32,
        pub(crate) header_crc_ok: bool,
        pub(crate) crc_ok: bool,
    }

    /// Bitwise CRC-16/ARC, as specified in the FIT protocol document.
    fn crc(data: &[u8]) -> u16 {
        let mut crc = 0u16;
        for &b in data {
            crc ^= u16::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xA001
                } else {
                    crc >> 1
                };
            }
        }
        crc
    }

    pub(crate) fn files(bytes: &[u8]) -> Result<Vec<File>, String> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let h = bytes.get(at..at + 12).ok_or("truncated header")?;
            let size = h[0];
            if (size != 12 && size != 14) || &h[8..12] != b".FIT" {
                return Err(format!("bad header at {at}"));
            }
            let data_size = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
            let header_crc_ok = if size == 14 {
                let stored = u16::from_le_bytes([bytes[at + 12], bytes[at + 13]]);
                stored == 0 || stored == crc(h)
            } else {
                true
            };
            let end = at + usize::from(size) + data_size as usize;
            let stored = bytes.get(end..end + 2).ok_or("truncated file")?;
            let crc_ok = u16::from_le_bytes([stored[0], stored[1]]) == crc(&bytes[at..end]);
            out.push(File {
                size,
                protocol_version: h[1],
                profile_version: u16::from_le_bytes([h[2], h[3]]),
                data_size,
                header_crc_ok,
                crc_ok,
            });
            at = end + 2;
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn crc_check_value() {
            assert_eq!(super::crc(b"123456789"), 0xBB3D);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv() {
        let rows = parse_csv("a,\"b,c\",\"d\"\"e\"\r\nData,0,x,\n");
        assert_eq!(rows[0], ["a", "b,c", "d\"e"]);
        assert_eq!(rows[1], ["Data", "0", "x", ""]);
    }

    #[test]
    fn unscaling() {
        assert_eq!(unscale("2.624", 1000.0, 0.0).unwrap(), json!(2624));
        assert_eq!(
            unscale("311.79999999999995", 5.0, 500.0).unwrap(),
            json!(4059)
        );
        assert_eq!(unscale("1.5", 1.0, 0.0).unwrap(), json!(1.5));
        assert!(unscale("1.5", 1000.0, 0.0).is_ok());
        assert!(unscale("1.00001", 1.0e3, 0.0).is_err());
    }
}
