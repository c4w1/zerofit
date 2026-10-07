//! Reads `Profile.xlsx` into a [`Profile`] subset.

use std::collections::BTreeSet;
use std::path::Path;

use calamine::{Data, Reader, Xlsx, open_workbook};

use crate::model::{EnumValue, Field, Message, Profile, SubField, TypeDef};

/// The messages `zerofit-profile` covers.
pub const MESSAGES: &[&str] = &[
    "file_id",
    "record",
    "lap",
    "session",
    "event",
    "device_info",
    "activity",
    "hrv",
];

/// Profile types with an integer base that are true enumerations (rather
/// than bit fields, indices or timestamps), so they get a Rust enum.
const INTEGER_ENUMS: &[&str] = &["manufacturer", "battery_status"];

/// Extracts the subset of the profile at `xlsx` needed for [`MESSAGES`].
pub fn extract(xlsx: &Path, sdk_version: &str) -> Result<Profile, String> {
    let mut workbook: Xlsx<_> =
        open_workbook(xlsx).map_err(|e| format!("cannot open {}: {e}", xlsx.display()))?;
    let types_sheet = workbook
        .worksheet_range("Types")
        .map_err(|e| format!("no Types sheet: {e}"))?;
    let messages_sheet = workbook
        .worksheet_range("Messages")
        .map_err(|e| format!("no Messages sheet: {e}"))?;

    let all_types = parse_types(types_sheet.rows().skip(1))?;
    let mesg_num = all_types
        .iter()
        .find(|t| t.name == "mesg_num")
        .ok_or("no mesg_num type")?
        .values
        .clone();

    let mut messages = Vec::new();
    for message in parse_messages(messages_sheet.rows().skip(1))? {
        if !MESSAGES.contains(&message.name.as_str()) {
            continue;
        }
        let number = mesg_num
            .iter()
            .find(|v| v.name == message.name)
            .ok_or_else(|| format!("{} missing from mesg_num", message.name))?
            .value;
        messages.push(Message {
            number: u16::try_from(number).map_err(|e| e.to_string())?,
            ..message
        });
    }
    if messages.len() != MESSAGES.len() {
        return Err(format!(
            "found {} of {} messages",
            messages.len(),
            MESSAGES.len()
        ));
    }

    // Keep the types used by main fields (subfield types such as
    // garmin_product are large and not generated).
    let used: BTreeSet<&str> = messages
        .iter()
        .flat_map(|m| &m.fields)
        .map(|f| f.field_type.as_str())
        .collect();
    let types = all_types
        .iter()
        .filter(|t| used.contains(t.name.as_str()))
        .cloned()
        .collect();

    Ok(Profile {
        sdk_version: sdk_version.to_owned(),
        mesg_num,
        types,
        messages,
    })
}

/// Whether `ty` becomes a Rust enum in the generated code.
#[must_use]
pub(crate) fn is_enum_type(ty: &TypeDef) -> bool {
    ty.base_type == "enum" || INTEGER_ENUMS.contains(&ty.name.as_str())
}

fn cell(row: &[Data], i: usize) -> Option<String> {
    let s = match row.get(i)? {
        Data::Empty => return None,
        Data::String(s) => s.trim().to_owned(),
        Data::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        other => other.to_string(),
    };
    (!s.is_empty()).then_some(s)
}

fn parse_number(s: &str) -> Result<u64, String> {
    let parsed = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => s.parse(),
    };
    parsed.map_err(|e| format!("bad number {s:?}: {e}"))
}

fn parse_list(s: Option<String>) -> Result<Vec<f64>, String> {
    s.map_or(Ok(Vec::new()), |s| {
        s.split(',')
            .map(|x| {
                x.trim()
                    .parse()
                    .map_err(|e| format!("bad number {x:?}: {e}"))
            })
            .collect()
    })
}

fn split(s: Option<String>) -> Vec<String> {
    s.map(|s| s.split(',').map(|x| x.trim().to_owned()).collect())
        .unwrap_or_default()
}

fn parse_types<'a>(rows: impl Iterator<Item = &'a [Data]>) -> Result<Vec<TypeDef>, String> {
    let mut types: Vec<TypeDef> = Vec::new();
    for row in rows {
        if let Some(name) = cell(row, 0) {
            types.push(TypeDef {
                name,
                base_type: cell(row, 1).ok_or("type without base type")?,
                values: Vec::new(),
            });
        } else if let (Some(name), Some(value), Some(ty)) =
            (cell(row, 2), cell(row, 3), types.last_mut())
        {
            ty.values.push(EnumValue {
                name,
                value: parse_number(&value)?,
                comment: cell(row, 4),
            });
        }
    }
    Ok(types)
}

fn parse_messages<'a>(rows: impl Iterator<Item = &'a [Data]>) -> Result<Vec<Message>, String> {
    let mut messages: Vec<Message> = Vec::new();
    for row in rows {
        if let Some(name) = cell(row, 0) {
            messages.push(Message {
                name,
                number: 0,
                fields: Vec::new(),
            });
            continue;
        }
        let (Some(name), Some(field_type), Some(message)) =
            (cell(row, 2), cell(row, 3), messages.last_mut())
        else {
            continue;
        };
        let scale = parse_list(cell(row, 6))?;
        let offset = parse_list(cell(row, 7))?;
        let units = cell(row, 8);
        if let Some(number) = cell(row, 1) {
            let number = u8::try_from(parse_number(&number)?).map_err(|e| e.to_string())?;
            message.fields.push(Field {
                number,
                name,
                field_type,
                array: cell(row, 4),
                components: split(cell(row, 5)),
                scale,
                offset,
                units,
                comment: cell(row, 13),
                subfields: Vec::new(),
            });
        } else if let Some(parent) = message.fields.last_mut() {
            let references = split(cell(row, 11))
                .into_iter()
                .zip(split(cell(row, 12)))
                .collect();
            parent.subfields.push(SubField {
                name,
                field_type,
                scale,
                offset,
                units,
                references,
            });
        }
    }
    Ok(messages)
}
