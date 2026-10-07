//! Removes personal data from a FIT file and re-encodes it with valid CRCs.
//!
//! ```sh
//! cargo run -p zerofit --example fit-anonymize -- in.fit out.fit
//! ```
//!
//! What it does:
//!
//! - **Moves every GPS coordinate.** The whole track is translated so its
//!   first position lands on a fixed anchor in the equatorial Pacific
//!   (0°N, 140°W). Distances, speeds and the track's shape are unchanged, so
//!   the file stays realistic test data. The translation depends on the
//!   original start point, which is not stored anywhere, so it cannot be
//!   undone from the output.
//! - **Clears device serial numbers** (`file_id.serial_number`,
//!   `device_info.serial_number`) and ANT+ device numbers by writing the
//!   invalid sentinel.
//! - **Clears developer fields named like serial numbers**, resolved
//!   through the file's `field_description` messages.
//! - **Drops personal messages** entirely: `user_profile` (name, weight,
//!   age, heart-rate zones) and `weight_scale`.
//! - **Safety net:** devices also write coordinates into undocumented
//!   fields (Wahoo, for example, stores them as semicircles in a private
//!   message and as decimal-degree *strings* in another). Any field not known
//!   to be a coordinate is set to invalid if it is a signed 32-bit value
//!   within a degree of the original track in semicircles, or a float or
//!   numeric string within a degree of it in degrees.
//!
//! Message and field numbers come from the FIT profile (Profile.xlsx 21.171,
//! fields with units "semicircles"). This example is the only place in the
//! raw `zerofit` crate that names profile numbers; it is a tool, not library
//! code.

// A command-line tool, not library code: counters and offsets here are
// bounded by the input size, and a panic would only abort this process.
#![allow(clippy::arithmetic_side_effects)]

use std::collections::BTreeSet;
use std::process::ExitCode;

use zerofit::encode::Encoder;
use zerofit::{BaseType, DataMessage, Decoder, Endian, Record, Value};

/// Latitude/longitude field pairs, as (global message, lat field, long field).
const COORDINATES: &[(u16, u8, u8)] = &[
    (18, 3, 4),    // session.start_position
    (18, 29, 30),  // session.nec
    (18, 31, 32),  // session.swc
    (18, 38, 39),  // session.end_position
    (19, 3, 4),    // lap.start_position
    (19, 5, 6),    // lap.end_position
    (20, 0, 1),    // record.position
    (32, 2, 3),    // course_point.position
    (128, 10, 11), // weather_conditions.observed_location
    (142, 3, 4),   // segment_lap.start_position
    (142, 5, 6),   // segment_lap.end_position
    (142, 25, 26), // segment_lap.nec
    (142, 27, 28), // segment_lap.swc
    (150, 1, 2),   // segment_point.position
    (160, 1, 2),   // gps_metadata.position
    (285, 5, 6),   // jump.position
    (312, 21, 22), // split.start_position
    (312, 23, 24), // split.end_position
    (317, 0, 1),   // climb_pro.position
];

/// Fields set to invalid, as (global message, field).
const CLEARED: &[(u16, u8)] = &[
    (0, 3),   // file_id.serial_number
    (23, 3),  // device_info.serial_number
    (23, 21), // device_info.ant_device_number
];

/// Messages removed entirely.
const DROPPED: &[u16] = &[
    3,  // user_profile
    30, // weight_scale
];

/// `field_description` message and its fields.
const FIELD_DESCRIPTION: u16 = 206;
const FD_DEVELOPER_DATA_INDEX: u8 = 0;
const FD_FIELD_DEFINITION_NUMBER: u8 = 1;
const FD_FIT_BASE_TYPE_ID: u8 = 2;
const FD_FIELD_NAME: u8 = 3;

/// Semicircles per degree: 2^31 / 180.
const SEMICIRCLES_PER_DEGREE: i64 = 11_930_465;
/// Where the first position of the track ends up: 0°N, 140°W.
const ANCHOR: (i32, i32) = (0, -1_670_265_100);

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: fit-anonymize <in.fit> <out.fit>");
        return ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {}: {e}", input.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };
    match anonymize(&bytes) {
        Ok((out, stats)) => {
            if let Err(e) = std::fs::write(output, out) {
                eprintln!("cannot write {}: {e}", output.to_string_lossy());
                return ExitCode::FAILURE;
            }
            eprintln!(
                "{}: moved {} coordinate pairs, cleared {} fields ({} by safety net), dropped {} messages",
                input.to_string_lossy(),
                stats.coordinates,
                stats.cleared,
                stats.suspicious,
                stats.dropped
            );
            for ((global, field), n) in &stats.suspicious_fields {
                eprintln!("  safety net: message {global} field {field}: {n} values");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot anonymize: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Default)]
struct Stats {
    coordinates: usize,
    cleared: usize,
    suspicious: usize,
    dropped: usize,
    /// (global message, field) cleared by the safety net, with counts.
    suspicious_fields: std::collections::BTreeMap<(u16, u8), usize>,
}

/// Bounding box of the original coordinates, in semicircles.
#[derive(Debug, Clone, Copy)]
struct Bounds {
    lat: (i32, i32),
    long: (i32, i32),
}

impl Bounds {
    /// Whether `v` (degrees) is within one degree of the latitude or
    /// longitude range.
    #[allow(clippy::cast_precision_loss)] // semicircles fit in f64 exactly
    fn near_degrees(&self, v: f64) -> bool {
        let d = SEMICIRCLES_PER_DEGREE as f64;
        let within =
            |(lo, hi): (i32, i32)| (f64::from(lo) / d - 1.0..=f64::from(hi) / d + 1.0).contains(&v);
        within(self.lat) || within(self.long)
    }

    /// Whether `v` (semicircles) is within one degree of the latitude or
    /// longitude range.
    fn near(&self, v: i32) -> bool {
        let v = i64::from(v);
        let within = |(lo, hi): (i32, i32)| {
            (i64::from(lo) - SEMICIRCLES_PER_DEGREE..=i64::from(hi) + SEMICIRCLES_PER_DEGREE)
                .contains(&v)
        };
        within(self.lat) || within(self.long)
    }
}

fn anonymize(bytes: &[u8]) -> Result<(Vec<u8>, Stats), Box<dyn std::error::Error>> {
    // Pass 1: find the first position and the extent of the track.
    let mut first = None;
    let mut bounds: Option<Bounds> = None;
    // Developer fields to clear: (developer data index, field number) -> base type.
    let mut serial_dev_fields = std::collections::BTreeMap::new();
    for record in Decoder::new(bytes) {
        if let Record::Data(msg) = record? {
            if let Some(key) = serial_developer_field(&msg) {
                serial_dev_fields.insert((key.0, key.1), key.2);
            }
            for (lat, long) in coordinate_pairs(&msg) {
                first.get_or_insert((lat, long));
                let b = bounds.get_or_insert(Bounds {
                    lat: (lat, lat),
                    long: (long, long),
                });
                b.lat = (b.lat.0.min(lat), b.lat.1.max(lat));
                b.long = (b.long.0.min(long), b.long.1.max(long));
            }
        }
    }
    let delta = first.map_or((0, 0), |(lat, long)| {
        (ANCHOR.0.wrapping_sub(lat), ANCHOR.1.wrapping_sub(long))
    });

    // Pass 2: rewrite.
    let mut stats = Stats::default();
    let mut enc = Encoder::new();
    for record in Decoder::new(bytes) {
        let msg = match record? {
            Record::Data(msg) => msg,
            other => {
                enc.write_record(&other)?;
                continue;
            }
        };
        let global = msg.global_message_number();
        if DROPPED.contains(&global) {
            stats.dropped += 1;
            continue;
        }
        let mut out = msg.bytes().to_vec();
        let coordinate_fields: BTreeSet<u8> = COORDINATES
            .iter()
            .filter(|(g, _, _)| *g == global)
            .flat_map(|&(_, lat, long)| [lat, long])
            .collect();
        for field in msg.fields() {
            let number = field.number();
            let range = field.offset()..field.offset() + field.bytes().len();
            let invalid = invalid_bytes(field.base_type(), field.endian(), field.bytes().len());
            if CLEARED.contains(&(global, number)) {
                out.splice(range, invalid);
                stats.cleared += 1;
            } else if coordinate_fields.contains(&number) {
                if let Some(Value::SInt32(v)) = field.value() {
                    let is_lat = COORDINATES
                        .iter()
                        .any(|&(g, lat, _)| g == global && lat == number);
                    let d = if is_lat { delta.0 } else { delta.1 };
                    out.splice(range, write_i32(v.wrapping_add(d), field.endian()));
                    stats.coordinates += usize::from(is_lat);
                }
            } else if let (Some(value), Some(b)) = (field.value(), bounds) {
                let looks_like_coordinate = match value {
                    Value::SInt32(v) => b.near(v),
                    Value::Float32(v) => b.near_degrees(f64::from(v)),
                    Value::Float64(v) => b.near_degrees(v),
                    Value::String(s) => s
                        .to_str()
                        .ok()
                        .and_then(|s| s.trim().parse::<f64>().ok())
                        .is_some_and(|v| b.near_degrees(v)),
                    _ => false,
                };
                if looks_like_coordinate {
                    out.splice(range, invalid);
                    stats.suspicious += 1;
                    *stats.suspicious_fields.entry((global, number)).or_default() += 1;
                    stats.cleared += 1;
                }
            }
        }
        let mut offset: usize = msg.fields().map(|f| f.bytes().len()).sum();
        for field in msg.developer_fields() {
            let len = field.bytes().len();
            let key = (field.developer_data_index(), field.number());
            if let Some(&base_type) = serial_dev_fields.get(&key) {
                let invalid = invalid_bytes(base_type, msg.definition().endian(), len);
                out.splice(offset..offset + len, invalid);
                stats.cleared += 1;
            }
            offset += len;
        }
        match msg.compressed_time_offset() {
            Some(t) => enc.write_compressed_data(msg.local_message_type(), t, &out)?,
            None => enc.write_data(msg.local_message_type(), &out)?,
        }
    }
    Ok((enc.finish()?, stats))
}

/// If `msg` is a `field_description` whose name mentions "serial", returns
/// (developer data index, field number, base type).
fn serial_developer_field(msg: &DataMessage<'_>) -> Option<(u8, u8, BaseType)> {
    if msg.global_message_number() != FIELD_DESCRIPTION {
        return None;
    }
    let byte = |n| match msg.field(n)?.value()? {
        Value::UInt8(v) | Value::Enum(v) => Some(v),
        _ => None,
    };
    let Value::String(name) = msg.field(FD_FIELD_NAME)?.value()? else {
        return None;
    };
    if !name.to_str().ok()?.to_ascii_lowercase().contains("serial") {
        return None;
    }
    let base_type = BaseType::from_byte(byte(FD_FIT_BASE_TYPE_ID)?).unwrap_or(BaseType::Byte);
    Some((
        byte(FD_DEVELOPER_DATA_INDEX)?,
        byte(FD_FIELD_DEFINITION_NUMBER)?,
        base_type,
    ))
}

/// Valid (lat, long) pairs in a message.
fn coordinate_pairs(msg: &DataMessage<'_>) -> Vec<(i32, i32)> {
    let global = msg.global_message_number();
    let read = |n| match msg.field(n).and_then(|f| f.value()) {
        Some(Value::SInt32(v)) => Some(v),
        _ => None,
    };
    COORDINATES
        .iter()
        .filter(|(g, _, _)| *g == global)
        .filter_map(|&(_, lat, long)| Some((read(lat)?, read(long)?)))
        .collect()
}

fn write_i32(v: i32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => v.to_le_bytes(),
        Endian::Big => v.to_be_bytes(),
    }
}

/// `len` bytes of `base_type`'s invalid sentinel, element by element, in
/// the field's byte order (signed types' sentinel is `MAX`, which is not
/// byte-order symmetric).
fn invalid_bytes(base_type: BaseType, endian: Endian, len: usize) -> Vec<u8> {
    let element: Vec<u8> = match (base_type, endian) {
        (BaseType::SInt8, _) => vec![0x7F],
        (BaseType::SInt16, Endian::Little) => i16::MAX.to_le_bytes().to_vec(),
        (BaseType::SInt16, Endian::Big) => i16::MAX.to_be_bytes().to_vec(),
        (BaseType::SInt32, Endian::Little) => i32::MAX.to_le_bytes().to_vec(),
        (BaseType::SInt32, Endian::Big) => i32::MAX.to_be_bytes().to_vec(),
        (BaseType::SInt64, Endian::Little) => i64::MAX.to_le_bytes().to_vec(),
        (BaseType::SInt64, Endian::Big) => i64::MAX.to_be_bytes().to_vec(),
        (
            BaseType::UInt8z
            | BaseType::UInt16z
            | BaseType::UInt32z
            | BaseType::UInt64z
            | BaseType::String,
            _,
        ) => vec![0x00],
        _ => vec![0xFF],
    };
    if len % element.len() == 0 {
        element.repeat(len / element.len())
    } else {
        vec![0xFF; len]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use zerofit::FieldDefinition;
    use zerofit::encode::FileOptions;

    const LAT: i32 = 429_009_715; // ~35.96°N
    const LONG: i32 = -1_001_358_950; // ~83.93°W

    fn sample() -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.begin_file(FileOptions::new(2132)).unwrap();
        let sint32 = BaseType::SInt32.to_byte();
        // file_id with a serial number.
        enc.write_definition(
            0,
            Endian::Little,
            0,
            &[FieldDefinition::new(3, 4, 0x8C)],
            &[],
        )
        .unwrap();
        enc.write_data(0, &12345u32.to_le_bytes()).unwrap();
        // user_profile: dropped.
        enc.write_definition(
            1,
            Endian::Little,
            3,
            &[FieldDefinition::new(0, 4, 0x07)],
            &[],
        )
        .unwrap();
        enc.write_data(1, b"Me\0\0").unwrap();
        // Two big-endian records with positions, plus an undocumented field
        // holding a latitude.
        let fields = [
            FieldDefinition::new(0, 4, sint32),
            FieldDefinition::new(1, 4, sint32),
            FieldDefinition::new(200, 4, sint32),
        ];
        enc.write_definition(2, Endian::Big, 20, &fields, &[])
            .unwrap();
        for step in [0, 1000] {
            let mut payload = Vec::new();
            payload.extend_from_slice(&(LAT + step).to_be_bytes());
            payload.extend_from_slice(&(LONG + step).to_be_bytes());
            payload.extend_from_slice(&LAT.to_be_bytes());
            enc.write_data(2, &payload).unwrap();
        }
        enc.finish().unwrap()
    }

    fn data(bytes: &[u8]) -> Vec<DataMessage<'_>> {
        Decoder::new(bytes)
            .filter_map(|r| match r.unwrap() {
                Record::Data(m) => Some(m),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn anonymizes() {
        let input = sample();
        let (out, stats) = anonymize(&input).unwrap();
        let msgs = data(&out); // also checks both CRCs
        assert_eq!(stats.dropped, 1);
        assert_eq!(msgs.len(), 3);
        assert!(msgs.iter().all(|m| m.global_message_number() != 3));
        assert_eq!(msgs[0].field(3).unwrap().value(), None, "serial cleared");

        let pos = |m: &DataMessage<'_>, n| match m.field(n).unwrap().value() {
            Some(Value::SInt32(v)) => v,
            other => panic!("{other:?}"),
        };
        // First point on the anchor, track shape preserved.
        assert_eq!((pos(&msgs[1], 0), pos(&msgs[1], 1)), ANCHOR);
        assert_eq!(pos(&msgs[2], 0) - pos(&msgs[1], 0), 1000);
        assert_eq!(pos(&msgs[2], 1) - pos(&msgs[1], 1), 1000);
        // Undocumented latitude caught by the safety net.
        assert_eq!(msgs[1].field(200).unwrap().value(), None);
        assert_eq!(stats.suspicious, 2);
    }

    #[test]
    fn invalid_sentinels_respect_byte_order() {
        assert_eq!(
            invalid_bytes(BaseType::SInt32, Endian::Big, 4),
            [0x7F, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            invalid_bytes(BaseType::SInt16, Endian::Little, 4),
            [0xFF, 0x7F, 0xFF, 0x7F]
        );
        assert_eq!(invalid_bytes(BaseType::UInt32z, Endian::Big, 4), [0; 4]);
        assert_eq!(invalid_bytes(BaseType::UInt16, Endian::Big, 3), [0xFF; 3]);
    }
}
