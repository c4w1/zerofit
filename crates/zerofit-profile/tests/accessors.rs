//! Typed accessors over synthetic messages written with the zerofit encoder.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use zerofit::encode::{Encoder, FileOptions};
use zerofit::{BaseType, DataMessage, Decoder, Endian, FieldDefinition, Record as RawRecord};
use zerofit_profile::messages::{Hrv, Record, Session};
use zerofit_profile::types::{Manufacturer, Sport};
use zerofit_profile::{Message, message_info, message_name};

fn f(number: u8, base_type: BaseType, count: u8) -> FieldDefinition {
    FieldDefinition::new(number, base_type.size() as u8 * count, base_type.to_byte())
}

fn build(endian: Endian) -> Vec<u8> {
    let le = |v: &[u8]| -> Vec<u8> {
        let mut v = v.to_vec();
        if endian == Endian::Big {
            v.reverse();
        }
        v
    };
    let mut enc = Encoder::new();
    enc.begin_file(FileOptions::new(2132)).unwrap();
    // record: timestamp, heart_rate, speed (scale 1000), altitude (5, 500).
    enc.write_definition(
        0,
        endian,
        20,
        &[
            f(253, BaseType::UInt32, 1),
            f(3, BaseType::UInt8, 1),
            f(6, BaseType::UInt16, 1),
            f(2, BaseType::UInt16, 1),
        ],
        &[],
    )
    .unwrap();
    let mut payload = le(&1_000_000_000u32.to_le_bytes());
    payload.push(150);
    payload.extend(le(&5432u16.to_le_bytes()));
    payload.extend(le(&3000u16.to_le_bytes()));
    enc.write_data(0, &payload).unwrap();
    // A record with every value invalid.
    enc.write_data(0, &[0xFF; 9]).unwrap();
    // session: sport = cycling (2), total_distance (scale 100).
    enc.write_definition(
        1,
        endian,
        18,
        &[f(5, BaseType::Enum, 1), f(9, BaseType::UInt32, 1)],
        &[],
    )
    .unwrap();
    let mut payload = vec![2];
    payload.extend(le(&1_234_567u32.to_le_bytes()));
    enc.write_data(1, &payload).unwrap();
    // hrv: time[3] (scale 1000), one element invalid.
    enc.write_definition(2, endian, 78, &[f(0, BaseType::UInt16, 3)], &[])
        .unwrap();
    let mut payload = le(&800u16.to_le_bytes());
    payload.extend([0xFF, 0xFF]);
    payload.extend(le(&1250u16.to_le_bytes()));
    enc.write_data(2, &payload).unwrap();
    // file_id: manufacturer garmin (1) written as uint16.
    enc.write_definition(3, endian, 0, &[f(1, BaseType::UInt16, 1)], &[])
        .unwrap();
    enc.write_data(3, &le(&1u16.to_le_bytes())).unwrap();
    enc.finish().unwrap()
}

fn messages(bytes: &[u8]) -> Vec<DataMessage<'_>> {
    Decoder::new(bytes)
        .filter_map(|r| match r.unwrap() {
            RawRecord::Data(m) => Some(m),
            _ => None,
        })
        .collect()
}

#[test]
fn typed_views_in_both_byte_orders() {
    for endian in [Endian::Little, Endian::Big] {
        let bytes = build(endian);
        let msgs = messages(&bytes);

        let r = Record::new(msgs[0]).unwrap();
        assert_eq!(r.timestamp(), Some(1_000_000_000));
        assert_eq!(r.heart_rate(), Some(150));
        assert_eq!(r.speed(), Some(5.432));
        assert_eq!(r.altitude(), Some(100.0));
        assert_eq!(r.cadence(), None, "absent field");

        let invalid = Record::new(msgs[1]).unwrap();
        assert_eq!(invalid.heart_rate(), None);
        assert_eq!(invalid.speed(), None);
        assert_eq!(invalid.timestamp(), None);

        let s = Session::new(msgs[2]).unwrap();
        assert_eq!(s.sport(), Some(Sport::Cycling));
        assert_eq!(s.total_distance(), Some(12_345.67));
        assert!(Record::new(msgs[2]).is_none());

        let h = Hrv::new(msgs[3]).unwrap();
        assert_eq!(h.time().collect::<Vec<_>>(), [0.8, 1.25]);

        match Message::new(msgs[4]) {
            Message::FileId(id) => assert_eq!(id.manufacturer(), Some(Manufacturer::Garmin)),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn enums_round_trip_and_keep_unknown_values() {
    assert_eq!(Sport::from_raw(2), Sport::Cycling);
    assert_eq!(Sport::Cycling.to_raw(), 2);
    assert_eq!(Sport::Cycling.name(), Some("cycling"));
    assert_eq!(Sport::from_raw(200), Sport::Unrecognized(200));
    assert_eq!(Sport::from_raw(200).to_raw(), 200);
    assert_eq!(Sport::Unrecognized(200).name(), None);
}

#[test]
fn metadata() {
    assert_eq!(message_name(18), Some("session"));
    assert_eq!(message_name(207), Some("developer_data_id"));
    let record = message_info(20).unwrap();
    assert_eq!(record.name(), "record");
    let hr = record.field(3).unwrap();
    assert_eq!(
        (hr.name(), hr.units(), hr.scale()),
        ("heart_rate", Some("bpm"), 1.0)
    );
    assert!(message_info(78).unwrap().field(0).unwrap().is_array());
    // Every covered message has metadata with unique field numbers.
    for m in zerofit_profile::MESSAGES {
        let mut numbers: Vec<_> = m
            .fields()
            .iter()
            .map(zerofit_profile::FieldInfo::number)
            .collect();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), m.fields().len(), "{}", m.name());
    }
}
