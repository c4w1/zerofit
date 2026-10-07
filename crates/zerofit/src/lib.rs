//! Zero-copy, `no_std`, panic-free decoder for FIT activity files.
//!
//! FIT (Flexible and Interoperable Data Transfer) is the binary format written
//! by GPS bike computers, running watches and other fitness devices.
//!
//! This crate is the *raw* layer: it understands the FIT protocol (file
//! headers, CRCs, definition and data messages, base types) but has no
//! knowledge of the FIT profile (message and field names, units, scaling).
//!
//! # Example
//!
//! ```
//! # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
//! use zerofit::{Decoder, Record, Value};
//!
//! // `bytes` is the contents of a .fit file. Nothing is copied or allocated:
//! // every message borrows from it.
//! for record in Decoder::new(bytes) {
//!     if let Record::Data(msg) = record? {
//!         // Global message 20 is `record`; field 3 is `heart_rate`.
//!         if msg.global_message_number() == 20 {
//!             if let Some(Value::UInt8(bpm)) = msg.field(3).and_then(|f| f.value()) {
//!                 println!("{:?}: {bpm} bpm", msg.timestamp());
//!             }
//!         }
//!     }
//! }
//! # Ok::<(), zerofit::Error>(())
//! ```
//!
//! # Guarantees
//!
//! - **No panics on any input.** Library code is compiled with lints that
//!   deny `unwrap`, `expect`, `panic!`, slice indexing and unchecked
//!   arithmetic, and the decoder is fuzzed.
//! - **No allocation.** The decoder's state is a fixed-size table; definitions
//!   and messages borrow from the input.
//! - **Precise errors.** Every [`Error`] carries the byte offset where decoding
//!   stopped.
//!
//! # Features
//!
//! - `std` (default): [`ReadDecoder`], which decodes from any
//!   `std::io::Read` with a bounded buffer. Implies `alloc`.
//! - `alloc`: enables the [`encode`] module (a minimal FIT writer).
//!
//! With `default-features = false` the crate is `#![no_std]` and allocation
//! free, and builds for bare-metal targets such as `thumbv7em-none-eabihf`.
//! [`Error`] implements [`core::error::Error`] in every configuration.
#![no_std]
// Test code may use arithmetic and casts freely. The non-test lib build (also
// checked by `clippy --all-targets`) still enforces these lints.
#![cfg_attr(
    test,
    allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)
)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

mod base_type;
pub mod crc;
mod decoder;
mod definition;
#[cfg(feature = "alloc")]
pub mod encode;
mod error;
mod header;
mod message;
#[cfg(feature = "std")]
mod read;
mod record_header;
mod value;

#[cfg(test)]
#[path = "../tests/common/builder.rs"]
mod test_builder;

pub use base_type::{BaseType, Endian};
pub use decoder::{DecodeOptions, Decoder, Record};
pub use definition::{
    Definition, DeveloperFieldDefinition, DeveloperFieldDefinitions, FieldDefinition,
    FieldDefinitions,
};
pub use error::{Error, ErrorKind};
pub use header::{FileHeader, ProtocolVersion};
pub use message::{DataMessage, DeveloperField, DeveloperFields, Field, Fields};
#[cfg(feature = "std")]
pub use read::{ReadDecoder, ReadError};
pub use value::{Array, ArrayIter, FitStr, Value};

#[cfg(test)]
mod tests {
    use crate::test_builder::{FitBuilder, HeaderCrc};

    #[test]
    fn builder_crc_matches_library_crc() {
        for data in [&b""[..], b"123456789", &[0xFF; 300]] {
            assert_eq!(crate::test_builder::crc16(data), crate::crc::crc16(data));
        }
    }

    #[test]
    fn builder_output_has_valid_header_and_file_crc() {
        let mut b = FitBuilder::new();
        b.definition(0, false, 0, &[(0, 1, 0)]).data(0, &[4]);
        let bytes = b.build();
        let header = crate::FileHeader::parse(&bytes).unwrap();
        assert_eq!(header.data_size() as usize, b.records_len());
        assert_eq!(bytes.len(), 14 + b.records_len() + 2);
        assert_eq!(crate::crc::crc16(&bytes), 0);

        let legacy = FitBuilder::new()
            .legacy_header()
            .header_crc(HeaderCrc::Zero)
            .build();
        assert_eq!(crate::FileHeader::parse(&legacy).unwrap().size(), 12);
    }
}
