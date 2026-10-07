//! Fuzzes the top-level slice decoder.
//!
//! Invariants checked on every input, with and without CRC validation (the
//! latter lets the fuzzer reach record parsing without forging CRCs):
//! - no panic, no infinite loop;
//! - every successful step advances the decoder's position;
//! - error offsets never point past the end of the input;
//! - after an error the decoder is fused.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zerofit::{DataMessage, DecodeOptions, Decoder, Record};

fuzz_target!(|data: &[u8]| {
    let lenient = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);
    for options in [DecodeOptions::new(), lenient] {
        let mut decoder = Decoder::with_options(data, options);
        let mut position = decoder.position();
        while let Some(record) = decoder.next() {
            match record {
                Ok(record) => {
                    assert!(decoder.position() > position, "no progress at {position}");
                    position = decoder.position();
                    if let Record::Data(message) = record {
                        touch(&message);
                    }
                }
                Err(e) => {
                    assert!(e.offset() <= data.len() as u64, "offset past end: {e:?}");
                    assert!(decoder.next().is_none(), "not fused after {e:?}");
                    break;
                }
            }
        }
    }
});

/// Decodes every field so the value paths are exercised too.
fn touch(message: &DataMessage<'_>) {
    let _ = message.timestamp();
    for field in message.fields() {
        let value = field.raw_value();
        let _ = value.is_invalid();
        let _ = value.as_f64();
        if let zerofit::Value::Array(array) = value {
            assert_eq!(array.iter().count(), array.len());
        }
        if let zerofit::Value::String(s) = value {
            let _ = s.to_str();
        }
    }
    for field in message.developer_fields() {
        let _ = field.bytes();
    }
}
