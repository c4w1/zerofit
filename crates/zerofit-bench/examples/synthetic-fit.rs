//! Writes a synthetic ride as a FIT activity file, for timing the full
//! pipeline (decode → analyze) in node and in the browser.
//!
//! ```sh
//! cargo run -p zerofit-bench --example synthetic-fit -- ride-4h.fit 14400
//! ```
//!
//! The ride is [`zerofit_bench::synthetic_ride`] (deterministic: intervals,
//! sprints, 1 % missing records, hourly stops), encoded as `file_id` plus
//! one `record` per sample with timestamp, power, heart rate, cadence,
//! speed, altitude and distance.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unwrap_used
)]

use zerofit::encode::{Encoder, FileOptions};
use zerofit::{BaseType, Endian, FieldDefinition};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "synthetic.fit".into());
    let seconds: u32 = args.next().map_or(14_400, |s| s.parse().unwrap());
    let records = zerofit_bench::synthetic_ride(seconds, 1);

    let f = |n, size, base: BaseType| FieldDefinition::new(n, size, base.to_byte());
    let mut enc = Encoder::new();
    enc.begin_file(FileOptions::new(21171)).unwrap();
    // file_id: type = activity (4), manufacturer = development (255).
    enc.write_definition(
        0,
        Endian::Little,
        0,
        &[f(0, 1, BaseType::Enum), f(1, 2, BaseType::UInt16)],
        &[],
    )
    .unwrap();
    enc.write_data(0, &[4, 255, 0]).unwrap();
    // record (20): timestamp, power, heart_rate, cadence, speed, altitude,
    // distance with the profile's scales.
    enc.write_definition(
        1,
        Endian::Little,
        20,
        &[
            f(253, 4, BaseType::UInt32),
            f(7, 2, BaseType::UInt16),
            f(3, 1, BaseType::UInt8),
            f(4, 1, BaseType::UInt8),
            f(6, 2, BaseType::UInt16),
            f(2, 2, BaseType::UInt16),
            f(5, 4, BaseType::UInt32),
        ],
        &[],
    )
    .unwrap();
    for r in &records {
        let mut d = Vec::with_capacity(16);
        d.extend_from_slice(&r.timestamp.to_le_bytes());
        d.extend_from_slice(&r.power.unwrap_or(0xFFFF).to_le_bytes());
        d.push(r.heart_rate.unwrap_or(0xFF));
        d.push(r.cadence.unwrap_or(0xFF));
        d.extend_from_slice(&((r.speed.unwrap_or(0.0) * 1000.0) as u16).to_le_bytes());
        d.extend_from_slice(&(((r.altitude.unwrap_or(0.0) + 500.0) * 5.0) as u16).to_le_bytes());
        d.extend_from_slice(&((r.distance.unwrap_or(0.0) * 100.0) as u32).to_le_bytes());
        enc.write_data(1, &d).unwrap();
    }
    enc.end_file().unwrap();
    let bytes = enc.finish().unwrap();
    std::fs::write(&out, &bytes).unwrap();
    println!("{out}: {} records, {} bytes", records.len(), bytes.len());
}
