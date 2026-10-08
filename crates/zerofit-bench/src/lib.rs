//! Workloads shared by the `decode` (criterion) and `alloc_count` benches.
//!
//! Each workload decodes a whole file and returns a checksum so the
//! optimizer cannot skip work. From least to most work:
//!
//! - [`records`]: walk every record (zerofit's lazy decoding: header parse
//!   and a slice split per message), CRC checked.
//! - [`all_fields`]: also decode every field value.
//! - [`profile_fields`]: also look up each field in the profile and apply
//!   scale/offset, which is what `fitparser` does for every field.
//! - [`stream`]: [`records`] through `ReadDecoder` from an in-memory reader.
//! - [`fitparser()`]: `fitparser::from_bytes`, which builds a `Vec` of records
//!   with named, scaled, owned field values.
//!
//! The `analytics` bench uses [`synthetic_ride`] and [`synthetic_daily_loads`]
//! for inputs longer than any fixture.

#![allow(clippy::cast_precision_loss, clippy::missing_panics_doc)]

use std::path::{Path, PathBuf};

use zerofit::{Decoder, ReadDecoder, Record, Value};

/// A fixture file loaded for benchmarking.
#[derive(Debug)]
pub struct Fixture {
    /// File stem, e.g. `icu_intervals`.
    pub name: String,
    /// File contents.
    pub bytes: Vec<u8>,
    /// Number of data messages.
    pub messages: u64,
}

/// The fixture files in `crates/zerofit/tests/fixtures`.
#[must_use]
pub fn fixtures() -> Vec<Fixture> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../zerofit/tests/fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "fit"));
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let bytes = std::fs::read(&p).ok()?;
            let name = p.file_stem()?.to_string_lossy().into_owned();
            let messages = Decoder::new(&bytes)
                .filter(|r| matches!(r, Ok(Record::Data(_))))
                .count() as u64;
            Some(Fixture {
                name,
                bytes,
                messages,
            })
        })
        .collect()
}

/// Walks every record.
#[must_use]
pub fn records(bytes: &[u8]) -> u64 {
    let mut sum = 0u64;
    for record in Decoder::new(bytes) {
        if let Ok(Record::Data(m)) = record {
            sum = sum.wrapping_add(u64::from(m.global_message_number()));
        }
    }
    sum
}

/// Walks every record and decodes every field value.
#[must_use]
pub fn all_fields(bytes: &[u8]) -> f64 {
    let mut sum = 0.0;
    for record in Decoder::new(bytes) {
        if let Ok(Record::Data(m)) = record {
            for field in m.fields() {
                sum += value_sum(field.value());
            }
        }
    }
    sum
}

/// Decodes every field value and applies the profile's scale and offset
/// where the profile layer knows the field.
#[must_use]
pub fn profile_fields(bytes: &[u8]) -> f64 {
    let mut sum = 0.0;
    for record in Decoder::new(bytes) {
        if let Ok(Record::Data(m)) = record {
            let info = zerofit_profile::message_info(m.global_message_number());
            for field in m.fields() {
                let value = field.value();
                let scaled = info
                    .and_then(|i| i.field(field.number()))
                    .zip(value)
                    .and_then(|(fi, v)| fi.scaled(&v));
                sum += scaled.unwrap_or_else(|| value_sum(value));
            }
        }
    }
    sum
}

/// [`records`] through the streaming decoder.
#[must_use]
pub fn stream(bytes: &[u8]) -> u64 {
    let mut sum = 0u64;
    let mut decoder = ReadDecoder::new(std::io::Cursor::new(bytes));
    while let Some(record) = decoder.next_record() {
        if let Ok(Record::Data(m)) = record {
            sum = sum.wrapping_add(u64::from(m.global_message_number()));
        }
    }
    sum
}

/// `fitparser::from_bytes`; returns the total number of fields (0 if it
/// fails, which it does not on the fixtures).
#[must_use]
pub fn fitparser(bytes: &[u8]) -> usize {
    fitparser::from_bytes(bytes).map_or(0, |records| records.iter().map(|r| r.fields().len()).sum())
}

fn value_sum(v: Option<Value<'_>>) -> f64 {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|e| e.as_f64()).sum(),
        Some(v) => v.as_f64().unwrap_or(1.0),
        None => 0.0,
    }
}

/// A deterministic synthetic ride of `seconds` records: a seeded LCG over
/// a structure of warm-up, 5-minute intervals at ~120 % FTP with 3-minute
/// recoveries, endurance riding and sprints, with heart rate following
/// power, speed, altitude, cadence, 1 % of records missing (smart
/// recording gaps) and a 2-minute stop every hour (a pause).
///
/// The first and last records are always present, so the ride spans
/// exactly `seconds` of elapsed time.
#[must_use]
// Synthetic values are in range by construction (power ≤ ~1200 W, HR < 255).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn synthetic_ride(seconds: u32, seed: u64) -> Vec<zerofit_analytics::resample::RawRecord> {
    let mut state = seed;
    let mut rand = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) as f64 / f64::from(1u32 << 31)
    };
    let mut records = Vec::with_capacity(seconds as usize);
    let mut hr = 100.0;
    let mut distance = 0.0;
    for t in 0..seconds {
        let in_hour = t % 3600;
        let last = t + 1 == seconds;
        let stopped = in_hour >= 3480 && t + 120 < seconds;
        if !last && (stopped || rand() < 0.01) {
            continue; // stop at the end of each hour, or a missed record
        }
        let block = (t / 480) % 6;
        let target = match (t, block) {
            (0..600, _) => 150.0,
            (_, 0 | 1) if (t % 480) < 300 => 300.0,
            _ if t % 1800 < 12 => 900.0,
            _ => 190.0,
        };
        let watts = (target * (0.85 + 0.3 * rand())).max(0.0);
        hr += (90.0 + watts * 0.3 - hr) / 40.0;
        let velocity = 6.0 + watts / 60.0 + rand();
        distance += velocity;
        records.push(zerofit_analytics::resample::RawRecord {
            timestamp: 1_000_000_000 + t,
            power: Some(watts as u16),
            heart_rate: Some(hr as u8),
            cadence: Some((80.0 + 15.0 * rand()) as u8),
            speed: Some(velocity),
            altitude: Some(200.0 + 50.0 * (f64::from(t) / 900.0).sin()),
            distance: Some(distance),
        });
    }
    records
}

/// `days` of daily training load: rest days, endurance days and a weekly
/// long ride, from a seeded LCG.
#[must_use]
pub fn synthetic_daily_loads(days: usize, seed: u64) -> Vec<f64> {
    let mut state = seed;
    (0..days)
        .map(|day| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let noise = (state >> 33) as f64 / f64::from(1u32 << 31);
            match day % 7 {
                0 => 0.0,
                6 => 180.0 + 60.0 * noise,
                _ => 50.0 + 60.0 * noise,
            }
        })
        .collect()
}
