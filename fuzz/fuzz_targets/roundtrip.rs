//! Decode, re-encode, decode again: the definitions and data messages must
//! be identical. Also runs every profile-layer accessor and developer field
//! resolution on each message.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zerofit::encode::Encoder;
use zerofit::{DecodeOptions, Decoder, Record};
use zerofit_profile::developer::DeveloperData;
use zerofit_profile::{Message, message_info, message_name};

fuzz_target!(|data: &[u8]| {
    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);

    // Re-encode every record that decodes.
    let mut original = Vec::new();
    let mut encoder = Encoder::new();
    let mut developer = DeveloperData::new();
    for record in Decoder::with_options(data, options) {
        let Ok(record) = record else { break };
        encoder
            .write_record(&record)
            .expect("decoded records always re-encode");
        match record {
            Record::Header(_) => developer.clear(),
            Record::Data(msg) => {
                profile(&msg, &mut developer);
                original.push(format!("{record:?}"));
            }
            Record::Definition(_) => original.push(format!("{record:?}")),
            Record::FileEnd { .. } => {}
        }
    }
    let encoded = encoder.finish().expect("finishing never fails here");
    if encoded.is_empty() {
        return; // nothing decoded; an empty input is (correctly) an error
    }

    // The re-encoded file has valid CRCs and the same content.
    let again: Vec<String> = Decoder::new(&encoded)
        .map(|r| r.expect("re-encoded files decode with CRC checks on"))
        .filter(|r| matches!(r, Record::Definition(_) | Record::Data(_)))
        .map(|r| format!("{r:?}"))
        .collect();
    assert_eq!(original, again);
});

fn profile(msg: &zerofit::DataMessage<'_>, developer: &mut DeveloperData) {
    let _ = message_name(msg.global_message_number());
    if let Some(info) = message_info(msg.global_message_number()) {
        for field in info.fields() {
            if let Some(raw) = msg.field(field.number()) {
                let _ = field.scaled(&raw.raw_value());
            }
        }
    }
    match Message::new(*msg) {
        Message::Record(r) => {
            let _ = (r.heart_rate(), r.speed(), r.altitude(), r.position_lat());
        }
        Message::Session(s) => {
            let _ = (s.sport(), s.total_distance(), s.avg_speed());
        }
        Message::Hrv(h) => {
            let _ = h.time().count();
        }
        Message::FileId(f) => {
            let _ = (f.manufacturer(), f.product_name(), f.type_());
        }
        _ => {}
    }
    developer.observe(msg);
    for field in developer.resolve(msg) {
        let _ = (field.value(), field.scaled(), field.description.name());
    }
}
