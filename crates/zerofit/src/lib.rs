//! Zero-copy, `no_std`, panic-free decoder for FIT activity files.
//!
//! FIT (Flexible and Interoperable Data Transfer) is the binary format written
//! by GPS bike computers, running watches and other fitness devices.
//!
//! This crate is the *raw* layer: it understands the FIT protocol (file
//! headers, CRCs, definition and data messages, base types) but has no
//! knowledge of the FIT profile (message and field names, units, scaling).
//!
//! # Features
//!
//! - `std` (default): `std::io` integration. Implies `alloc`.
//! - `alloc`: APIs that need a heap.
//!
//! With `default-features = false` the crate is `#![no_std]` and allocation
//! free.
#![no_std]
// Test code may use arithmetic freely. The non-test lib build (also checked
// by `clippy --all-targets`) still enforces the lint.
#![cfg_attr(test, allow(clippy::arithmetic_side_effects))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

mod base_type;
pub mod crc;
// Temporarily unused outside tests; wired into the decoder in the next commit.
#[allow(dead_code)]
mod definition;
mod error;
mod header;
#[allow(dead_code)]
mod record_header;
mod value;

#[cfg(test)]
#[path = "../tests/common/builder.rs"]
mod test_builder;

pub use base_type::{BaseType, Endian};
pub use definition::{
    Definition, DeveloperFieldDefinition, DeveloperFieldDefinitions, FieldDefinition,
    FieldDefinitions,
};
pub use error::{Error, ErrorKind};
pub use header::{FileHeader, ProtocolVersion};
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
