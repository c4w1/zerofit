//! Developer field resolution over a synthetic file.

#![cfg(feature = "alloc")]
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::float_cmp,
    clippy::indexing_slicing
)]

use zerofit::encode::{Encoder, FileOptions};
use zerofit::{
    BaseType, Decoder, DeveloperFieldDefinition, Endian, FieldDefinition, Record, Value,
};
use zerofit_profile::developer::DeveloperData;

fn file(endian: Endian) -> Vec<u8> {
    let be = endian == Endian::Big;
    let u16b = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut enc = Encoder::new();
    enc.begin_file(FileOptions::new(2132)).unwrap();
    // developer_data_id: application_id[16], developer_data_index.
    enc.write_definition(
        0,
        endian,
        207,
        &[
            FieldDefinition::new(1, 16, 0x0D),
            FieldDefinition::new(3, 1, 0x02),
        ],
        &[],
    )
    .unwrap();
    let mut payload = vec![0xAB; 16];
    payload.push(0);
    enc.write_data(0, &payload).unwrap();
    // field_description: index 0, field 5, uint16, "Glucose", scale 10, offset 1, "mg/dL".
    enc.write_definition(
        1,
        endian,
        206,
        &[
            FieldDefinition::new(0, 1, 0x02),
            FieldDefinition::new(1, 1, 0x02),
            FieldDefinition::new(2, 1, 0x02),
            FieldDefinition::new(3, 8, 0x07),
            FieldDefinition::new(6, 1, 0x02),
            FieldDefinition::new(7, 1, 0x01),
            FieldDefinition::new(8, 6, 0x07),
        ],
        &[],
    )
    .unwrap();
    let mut payload = vec![0, 5, BaseType::UInt16.to_byte()];
    payload.extend_from_slice(b"Glucose\0");
    payload.extend([10, 1]);
    payload.extend_from_slice(b"mg/dL\0");
    enc.write_data(1, &payload).unwrap();
    // record: heart_rate + developer fields 5 (declared) and 6 (not declared).
    enc.write_definition(
        2,
        endian,
        20,
        &[FieldDefinition::new(3, 1, 0x02)],
        &[
            DeveloperFieldDefinition::new(5, 2, 0),
            DeveloperFieldDefinition::new(6, 1, 0),
        ],
    )
    .unwrap();
    let mut payload = vec![140];
    payload.extend(u16b(1015));
    payload.push(9);
    enc.write_data(2, &payload).unwrap();
    enc.finish().unwrap()
}

#[test]
fn resolves_declared_fields_in_both_byte_orders() {
    for endian in [Endian::Little, Endian::Big] {
        let bytes = file(endian);
        let mut dev = DeveloperData::new();
        let mut resolved = Vec::new();
        for r in Decoder::new(&bytes) {
            if let Record::Data(msg) = r.unwrap() {
                dev.observe(&msg);
                for f in dev.resolve(&msg) {
                    resolved.push((
                        f.description.name().unwrap().to_owned(),
                        f.value(),
                        f.scaled(),
                        f.description.units().map(str::to_owned),
                    ));
                }
            }
        }
        assert_eq!(
            resolved,
            [(
                "Glucose".to_owned(),
                Some(Value::UInt16(1015)),
                Some(100.5),
                Some("mg/dL".to_owned())
            )]
        );
        assert_eq!(dev.application(0).unwrap().application_id, Some([0xAB; 16]));
        let d = dev.description(0, 5).unwrap();
        assert_eq!((d.scale(), d.offset()), (Some(10), Some(1)));
        dev.clear();
        assert!(dev.description(0, 5).is_none());
    }
}
