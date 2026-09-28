//! The FIT file header.
//!
//! Every FIT file starts with a 12- or 14-byte header:
//!
//! | Offset | Size | Field                                              |
//! |-------:|-----:|----------------------------------------------------|
//! | 0      | 1    | Header size: 12 (legacy) or 14                     |
//! | 1      | 1    | Protocol version (high nibble major, low minor)    |
//! | 2      | 2    | Profile version, little-endian                     |
//! | 4      | 4    | Data size: bytes of records after the header, LE   |
//! | 8      | 4    | ASCII `.FIT`                                       |
//! | 12     | 2    | CRC of bytes 0..12, LE (14-byte headers only)      |
//!
//! A stored header CRC of `0x0000` means "not computed" and is not checked.
//! The records are followed by a 2-byte file CRC that is not counted in the
//! data size.

use crate::crc::crc16;
use crate::error::{Error, ErrorKind};

/// Size of a legacy file header, without CRC.
const LEGACY_SIZE: u8 = 12;
/// Size of a current file header, with CRC.
const CRC_SIZE: u8 = 14;
/// The data type signature at bytes 8..12.
const SIGNATURE: [u8; 4] = *b".FIT";

/// A parsed and validated FIT file header.
///
/// ```
/// use zerofit::FileHeader;
///
/// // A 14-byte header: protocol 2.0, profile 21.32, 100 data bytes.
/// let mut bytes = vec![14, 0x20];
/// bytes.extend_from_slice(&2132u16.to_le_bytes());
/// bytes.extend_from_slice(&100u32.to_le_bytes());
/// bytes.extend_from_slice(b".FIT");
/// let crc = zerofit::crc::crc16(&bytes);
/// bytes.extend_from_slice(&crc.to_le_bytes());
///
/// let header = FileHeader::parse(&bytes)?;
/// assert_eq!(header.size(), 14);
/// assert_eq!(header.protocol_version().major(), 2);
/// assert_eq!(header.profile_version(), 2132);
/// assert_eq!(header.data_size(), 100);
/// assert_eq!(header.crc(), Some(crc));
/// # Ok::<(), zerofit::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileHeader {
    size: u8,
    protocol_version: ProtocolVersion,
    profile_version: u16,
    data_size: u32,
    crc: Option<u16>,
}

impl FileHeader {
    /// Parses and validates a file header at the start of `bytes`, including
    /// its CRC if present and non-zero.
    ///
    /// Bytes after the header are ignored. Error offsets are relative to the
    /// start of `bytes`.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidHeaderSize`] if the first byte is not 12 or 14.
    /// - [`ErrorKind::UnexpectedEof`] if `bytes` is shorter than the header.
    /// - [`ErrorKind::InvalidSignature`] if bytes 8..12 are not `.FIT`.
    /// - [`ErrorKind::HeaderCrcMismatch`] if the header CRC is wrong.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        Self::parse_at(bytes, 0, true)
    }

    /// Parses a header at the start of `bytes`, reporting errors relative to
    /// absolute stream offset `base`. Skips the CRC check if `check_crc` is
    /// false.
    pub(crate) fn parse_at(bytes: &[u8], base: u64, check_crc: bool) -> Result<Self, Error> {
        let err = |kind| Error::new(kind, base);

        let Some(&size) = bytes.first() else {
            return Err(err(ErrorKind::UnexpectedEof {
                needed: usize::from(LEGACY_SIZE),
            }));
        };
        if size != LEGACY_SIZE && size != CRC_SIZE {
            return Err(err(ErrorKind::InvalidHeaderSize(size)));
        }
        let Some(head) = bytes.first_chunk::<12>() else {
            return Err(err(ErrorKind::UnexpectedEof {
                needed: usize::from(size).saturating_sub(bytes.len()),
            }));
        };
        let [_, protocol, p0, p1, d0, d1, d2, d3, s0, s1, s2, s3] = *head;

        let crc = if size == CRC_SIZE {
            let Some(&[c0, c1]) = bytes.get(12..14).and_then(|s| s.first_chunk::<2>()) else {
                return Err(err(ErrorKind::UnexpectedEof {
                    needed: usize::from(CRC_SIZE).saturating_sub(bytes.len()),
                }));
            };
            Some(u16::from_le_bytes([c0, c1]))
        } else {
            None
        };

        let signature = [s0, s1, s2, s3];
        if signature != SIGNATURE {
            return Err(err(ErrorKind::InvalidSignature(signature)));
        }

        if check_crc {
            if let Some(stored) = crc.filter(|&c| c != 0) {
                let computed = crc16(head);
                if stored != computed {
                    return Err(err(ErrorKind::HeaderCrcMismatch { stored, computed }));
                }
            }
        }

        Ok(Self {
            size,
            protocol_version: ProtocolVersion(protocol),
            profile_version: u16::from_le_bytes([p0, p1]),
            data_size: u32::from_le_bytes([d0, d1, d2, d3]),
            crc,
        })
    }

    /// Header size in bytes: 12 or 14.
    #[must_use]
    pub const fn size(&self) -> u8 {
        self.size
    }

    /// FIT protocol version the file was written with.
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    /// FIT profile version the file was written with, as stored (for example
    /// `2132` for profile 21.32).
    #[must_use]
    pub const fn profile_version(&self) -> u16 {
        self.profile_version
    }

    /// Number of record bytes following the header, excluding the 2-byte file
    /// CRC.
    #[must_use]
    pub const fn data_size(&self) -> u32 {
        self.data_size
    }

    /// Stored header CRC: `None` for 12-byte headers. `Some(0)` means the
    /// writer did not compute one.
    #[must_use]
    pub const fn crc(&self) -> Option<u16> {
        self.crc
    }
}

/// A FIT protocol version, stored as one byte: major version in the high
/// nibble, minor in the low nibble.
///
/// ```
/// let v = zerofit::ProtocolVersion::from_byte(0x20);
/// assert_eq!((v.major(), v.minor()), (2, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion(u8);

impl ProtocolVersion {
    /// Wraps a raw protocol version byte.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Self {
        Self(byte)
    }

    /// The raw byte.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self.0
    }

    /// Major version (high nibble).
    #[must_use]
    pub const fn major(self) -> u8 {
        self.0 >> 4
    }

    /// Minor version (low nibble).
    #[must_use]
    pub const fn minor(self) -> u8 {
        self.0 & 0x0F
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn header12(data_size: u32) -> Vec<u8> {
        let mut b = std::vec![12, 0x10];
        b.extend_from_slice(&2132u16.to_le_bytes());
        b.extend_from_slice(&data_size.to_le_bytes());
        b.extend_from_slice(b".FIT");
        b
    }

    fn header14(data_size: u32) -> Vec<u8> {
        let mut b = header12(data_size);
        b[0] = 14;
        b[1] = 0x20;
        let crc = crc16(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        b
    }

    fn kind(bytes: &[u8]) -> ErrorKind {
        FileHeader::parse(bytes).unwrap_err().kind()
    }

    #[test]
    fn parses_12_byte_header() {
        let h = FileHeader::parse(&header12(1234)).unwrap();
        assert_eq!(h.size(), 12);
        assert_eq!(h.protocol_version(), ProtocolVersion::from_byte(0x10));
        assert_eq!(h.profile_version(), 2132);
        assert_eq!(h.data_size(), 1234);
        assert_eq!(h.crc(), None);
    }

    #[test]
    fn parses_14_byte_header_with_crc() {
        let bytes = header14(0xDEAD_BEEF);
        let h = FileHeader::parse(&bytes).unwrap();
        assert_eq!(h.size(), 14);
        assert_eq!(h.protocol_version().major(), 2);
        assert_eq!(h.data_size(), 0xDEAD_BEEF);
        assert_eq!(h.crc(), Some(u16::from_le_bytes([bytes[12], bytes[13]])));
    }

    #[test]
    fn zero_crc_is_not_checked() {
        let mut bytes = header14(10);
        bytes[12] = 0;
        bytes[13] = 0;
        assert_eq!(FileHeader::parse(&bytes).unwrap().crc(), Some(0));
    }

    #[test]
    fn crc_mismatch_is_reported() {
        let mut bytes = header14(10);
        let computed = u16::from_le_bytes([bytes[12], bytes[13]]);
        bytes[12] ^= 0x01;
        let stored = u16::from_le_bytes([bytes[12], bytes[13]]);
        assert_eq!(
            kind(&bytes),
            ErrorKind::HeaderCrcMismatch { stored, computed }
        );
    }

    #[test]
    fn corrupted_body_fails_crc() {
        let mut bytes = header14(10);
        bytes[4] ^= 0x80;
        assert!(matches!(kind(&bytes), ErrorKind::HeaderCrcMismatch { .. }));
    }

    #[test]
    fn crc_check_can_be_skipped() {
        let mut bytes = header14(10);
        bytes[12] ^= 0x01;
        assert!(FileHeader::parse_at(&bytes, 0, false).is_ok());
    }

    #[test]
    fn rejects_bad_sizes() {
        for size in [0u8, 1, 11, 13, 15, 255] {
            let mut bytes = header14(0);
            bytes[0] = size;
            assert_eq!(kind(&bytes), ErrorKind::InvalidHeaderSize(size), "{size}");
        }
    }

    #[test]
    fn rejects_bad_signature() {
        let mut bytes = header12(0);
        bytes[8..12].copy_from_slice(b"FIT.");
        assert_eq!(kind(&bytes), ErrorKind::InvalidSignature(*b"FIT."));
    }

    #[test]
    fn empty_input_needs_12_bytes() {
        assert_eq!(kind(&[]), ErrorKind::UnexpectedEof { needed: 12 });
    }

    #[test]
    fn every_truncation_of_12_byte_header() {
        let bytes = header12(0);
        for len in 1..12 {
            assert_eq!(
                kind(&bytes[..len]),
                ErrorKind::UnexpectedEof { needed: 12 - len },
                "len {len}"
            );
        }
    }

    #[test]
    fn every_truncation_of_14_byte_header() {
        let bytes = header14(0);
        for len in 1..14 {
            assert_eq!(
                kind(&bytes[..len]),
                ErrorKind::UnexpectedEof { needed: 14 - len },
                "len {len}"
            );
        }
    }

    #[test]
    fn trailing_bytes_are_ignored() {
        let mut bytes = header14(3);
        bytes.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert_eq!(FileHeader::parse(&bytes).unwrap().data_size(), 3);
    }

    #[test]
    fn errors_use_base_offset() {
        let mut bytes = header12(0);
        bytes[8] = b'X';
        let err = FileHeader::parse_at(&bytes, 5000, true).unwrap_err();
        assert_eq!(err.offset(), 5000);
    }

    #[test]
    fn protocol_version_nibbles() {
        let v = ProtocolVersion::from_byte(0x2A);
        assert_eq!(v.major(), 2);
        assert_eq!(v.minor(), 10);
        assert_eq!(v.to_byte(), 0x2A);
        assert!(ProtocolVersion::from_byte(0x10) < ProtocolVersion::from_byte(0x20));
    }
}
