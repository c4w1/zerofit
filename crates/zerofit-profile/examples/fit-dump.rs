//! Dumps a FIT file as JSON or CSV.
//!
//! ```sh
//! cargo run -p zerofit-profile --example fit-dump -- ride.fit            # JSON
//! cargo run -p zerofit-profile --example fit-dump -- --csv ride.fit      # CSV
//! cargo run -p zerofit-profile --example fit-dump -- --raw ride.fit      # no profile
//! ```
//!
//! The file is decoded with `ReadDecoder`, so memory use stays bounded
//! however large the file is, and output is written as messages arrive.
//!
//! - **JSON**: an array with one object per data message:
//!   `{"message": "record", "global": 20, "timestamp": 1000000000,
//!   "fields": {"heart_rate": {"value": 142, "units": "bpm"}, ...},
//!   "developer_fields": {...}}`. Fields the profile does not name are
//!   keyed by number. Values are scaled to profile units; enums are raw
//!   numbers.
//! - **CSV**: one row per message in the layout of Garmin's FitCSVTool
//!   (`Type,Local Number,Message,Field 1,Value 1,Units 1,...`), definitions
//!   included, so the two can be compared.
//! - `--raw` skips the profile: names become numbers and values stay
//!   unscaled.

// A command-line tool, not library code.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::float_cmp // exact comparisons with the profile's 1.0 / 0.0 defaults
)]

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::process::ExitCode;

use serde_json::{Map, Value as Json, json};
use zerofit::{DataMessage, ReadDecoder, Record, Value};
use zerofit_profile::developer::DeveloperData;
use zerofit_profile::{message_info, message_name};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Csv,
}

fn main() -> ExitCode {
    let mut format = Format::Json;
    let mut raw = false;
    let mut path = None;
    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--json") => format = Format::Json,
            Some("--csv") => format = Format::Csv,
            Some("--raw") => raw = true,
            Some("-h" | "--help") => {
                eprintln!("usage: fit-dump [--json | --csv] [--raw] <file.fit>");
                return ExitCode::SUCCESS;
            }
            _ => path = Some(arg),
        }
    }
    let Some(path) = path else {
        eprintln!("usage: fit-dump [--json | --csv] [--raw] <file.fit>");
        return ExitCode::FAILURE;
    };
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("cannot open {}: {e}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    match dump(BufReader::new(file), &mut out, format, raw).and_then(|()| Ok(out.flush()?)) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader of our output went away (e.g. `fit-dump x.fit | head`).
        Err(e) if is_broken_pipe(e.as_ref()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(e: &(dyn std::error::Error + 'static)) -> bool {
    let kind = match (
        e.downcast_ref::<io::Error>(),
        e.downcast_ref::<serde_json::Error>(),
    ) {
        (Some(io), _) => Some(io.kind()),
        (None, Some(json)) => json.io_error_kind(),
        (None, None) => None,
    };
    kind == Some(io::ErrorKind::BrokenPipe)
}

fn dump(
    input: impl io::Read,
    out: &mut impl Write,
    format: Format,
    raw: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = ReadDecoder::new(input);
    let mut developer = DeveloperData::new();
    let mut first = true;
    if format == Format::Json {
        writeln!(out, "[")?;
    } else {
        write!(out, "Type,Local Number,Message")?;
        for i in 1..=32 {
            write!(out, ",Field {i},Value {i},Units {i}")?;
        }
        writeln!(out)?;
    }
    while let Some(record) = decoder.next_record() {
        match record? {
            Record::Header(_) => developer.clear(),
            Record::Definition(d) if format == Format::Csv => {
                let global = d.global_message_number();
                let info = (!raw).then(|| message_info(global)).flatten();
                write!(
                    out,
                    "Definition,{},{}",
                    d.local_message_type(),
                    name(global, raw)
                )?;
                for f in d.fields() {
                    let fname = info
                        .and_then(|i| i.field(f.number()))
                        .map_or_else(|| f.number().to_string(), |fi| fi.name().to_owned());
                    write!(out, ",{fname},{},", f.size())?;
                }
                writeln!(out)?;
            }
            Record::Data(msg) => {
                developer.observe(&msg);
                match format {
                    Format::Json => {
                        if !first {
                            writeln!(out, ",")?;
                        }
                        first = false;
                        serde_json::to_writer(&mut *out, &to_json(&msg, &developer, raw))?;
                    }
                    Format::Csv => write_csv(out, &msg, &developer, raw)?,
                }
            }
            Record::Definition(_) | Record::FileEnd { .. } => {}
        }
    }
    if format == Format::Json {
        writeln!(out, "\n]")?;
    }
    Ok(())
}

fn name(global: u16, raw: bool) -> String {
    (!raw)
        .then(|| message_name(global))
        .flatten()
        .map_or_else(|| global.to_string(), str::to_owned)
}

/// A field as (name, value, units), scaled through the profile unless raw.
fn fields(msg: &DataMessage<'_>, raw: bool) -> Vec<(String, Json, Option<&'static str>)> {
    let info = (!raw)
        .then(|| message_info(msg.global_message_number()))
        .flatten();
    msg.fields()
        .filter_map(|field| {
            let value = field.value()?;
            let fi = info.and_then(|i| i.field(field.number()));
            let name = fi.map_or_else(|| field.number().to_string(), |fi| fi.name().to_owned());
            let json = match fi {
                Some(fi) if fi.scale() != 1.0 || fi.offset() != 0.0 => {
                    scaled_json(&value, |v| v / fi.scale() - fi.offset())
                }
                _ => value_json(&value),
            };
            Some((name, json, fi.and_then(zerofit_profile::FieldInfo::units)))
        })
        .collect()
}

fn to_json(msg: &DataMessage<'_>, developer: &DeveloperData, raw: bool) -> Json {
    let mut obj = Map::new();
    obj.insert(
        "message".into(),
        json!(name(msg.global_message_number(), raw)),
    );
    obj.insert("global".into(), json!(msg.global_message_number()));
    if let Some(ts) = msg.timestamp() {
        obj.insert("timestamp".into(), json!(ts));
    }
    let mut fs = Map::new();
    for (name, value, units) in fields(msg, raw) {
        let mut f = Map::new();
        f.insert("value".into(), value);
        if let Some(u) = units {
            f.insert("units".into(), json!(u));
        }
        fs.insert(name, Json::Object(f));
    }
    obj.insert("fields".into(), Json::Object(fs));
    let mut dev = Map::new();
    for field in developer.resolve(msg) {
        let Some(value) = field.value() else { continue };
        let name = field.description.name().map_or_else(
            || {
                format!(
                    "{}:{}",
                    field.field.developer_data_index(),
                    field.field.number()
                )
            },
            str::to_owned,
        );
        let mut f = Map::new();
        let v = if raw {
            value_json(&value)
        } else {
            field
                .scaled()
                .map_or_else(|| value_json(&value), |v| json!(v))
        };
        f.insert("value".into(), v);
        if let Some(u) = field.description.units() {
            f.insert("units".into(), json!(u));
        }
        dev.insert(name, Json::Object(f));
    }
    if !dev.is_empty() {
        obj.insert("developer_fields".into(), Json::Object(dev));
    }
    Json::Object(obj)
}

fn write_csv(
    out: &mut impl Write,
    msg: &DataMessage<'_>,
    developer: &DeveloperData,
    raw: bool,
) -> io::Result<()> {
    write!(
        out,
        "Data,{},{}",
        msg.local_message_type(),
        name(msg.global_message_number(), raw)
    )?;
    for (name, value, units) in fields(msg, raw) {
        write!(out, ",{name},{},{}", csv_value(&value), units.unwrap_or(""))?;
    }
    for field in developer.resolve(msg) {
        let Some(value) = field.value() else { continue };
        let v = field
            .scaled()
            .map_or_else(|| value_json(&value), |v| json!(v));
        write!(
            out,
            ",{},{},{}",
            field.description.name().unwrap_or("developer"),
            csv_value(&v),
            field.description.units().unwrap_or("")
        )?;
    }
    writeln!(out)
}

/// FitCSVTool style: quoted, arrays joined with `|`.
fn csv_value(v: &Json) -> String {
    let text = match v {
        Json::Array(a) => a.iter().map(plain).collect::<Vec<_>>().join("|"),
        other => plain(other),
    };
    format!("\"{}\"", text.replace('"', "\"\""))
}

fn plain(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn value_json(v: &Value<'_>) -> Json {
    match v {
        Value::String(s) => json!(s.to_str().unwrap_or("")),
        Value::Bytes(b) => json!(b),
        Value::Array(a) => Json::Array(
            a.iter()
                .map(|e| {
                    if e.is_invalid() {
                        Json::Null
                    } else {
                        value_json(&e)
                    }
                })
                .collect(),
        ),
        Value::Float32(f) => json!(f),
        Value::Float64(f) => json!(f),
        Value::UInt64(n) | Value::UInt64z(n) => json!(n),
        other => other.as_i64().map_or(Json::Null, |n| json!(n)),
    }
}

fn scaled_json(v: &Value<'_>, scale: impl Fn(f64) -> f64) -> Json {
    match v {
        Value::Array(a) => Json::Array(
            a.iter()
                .map(|e| {
                    if e.is_invalid() {
                        Json::Null
                    } else {
                        e.as_f64().map_or(Json::Null, |x| json!(scale(x)))
                    }
                })
                .collect(),
        ),
        other => other
            .as_f64()
            .map_or_else(|| value_json(other), |x| json!(scale(x))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../zerofit/tests/fixtures")
            .join(name);
        std::fs::read(path).unwrap()
    }

    #[test]
    fn json_output_is_valid_and_scaled() {
        let mut out = Vec::new();
        dump(&fixture("icu_short.fit")[..], &mut out, Format::Json, false).unwrap();
        let parsed: Json = serde_json::from_slice(&out).unwrap();
        let messages = parsed.as_array().unwrap();
        assert_eq!(messages.len(), 96);
        let record = messages.iter().find(|m| m["message"] == "record").unwrap();
        assert_eq!(record["fields"]["heart_rate"]["units"], "bpm");
        // altitude raw 3733 -> 3733 / 5 - 500 = 246.6 m
        assert_eq!(record["fields"]["altitude"]["value"], json!(246.6));
    }

    #[test]
    fn csv_output_has_one_row_per_record() {
        let mut out = Vec::new();
        dump(&fixture("icu_short.fit")[..], &mut out, Format::Csv, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("Data,")).count(), 96);
        assert_eq!(
            text.lines()
                .filter(|l| l.starts_with("Definition,"))
                .count(),
            8
        );
        assert!(text.contains(",heart_rate,\"99\",bpm"));
    }

    #[test]
    fn raw_mode_uses_numbers() {
        let mut out = Vec::new();
        dump(&fixture("icu_short.fit")[..], &mut out, Format::Json, true).unwrap();
        let parsed: Json = serde_json::from_slice(&out).unwrap();
        let record = parsed
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["global"] == 20)
            .unwrap();
        assert_eq!(record["message"], "20");
        assert_eq!(record["fields"]["2"]["value"], json!(3733));
    }
}
