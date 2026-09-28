//! The one-byte header that starts every record.
//!
//! ```text
//! Normal header:                 Compressed timestamp header:
//!   bit 7    0                     bit 7     1
//!   bit 6    1 = definition        bits 5-6  local message type (0-3)
//!   bit 5    developer data flag   bits 0-4  time offset (seconds)
//!   bit 4    reserved
//!   bits 0-3 local message type
//! ```

/// A decoded record header byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecordHeader {
    /// A definition message for `local` message type.
    Definition {
        /// Local message type being defined (0-15).
        local: u8,
        /// Whether developer field definitions follow the regular ones.
        has_developer_fields: bool,
    },
    /// A data message using the definition for `local`.
    Data {
        /// Local message type (0-15).
        local: u8,
    },
    /// A data message with a compressed timestamp.
    CompressedTimestamp {
        /// Local message type (0-3).
        local: u8,
        /// Low 5 bits of the message's timestamp, in seconds.
        time_offset: u8,
    },
}

impl RecordHeader {
    const COMPRESSED: u8 = 0x80;
    const DEFINITION: u8 = 0x40;
    const DEVELOPER_DATA: u8 = 0x20;
    const LOCAL_MASK: u8 = 0x0F;
    const COMPRESSED_LOCAL_SHIFT: u8 = 5;
    const COMPRESSED_LOCAL_MASK: u8 = 0x03;
    const TIME_OFFSET_MASK: u8 = 0x1F;

    /// Decodes a record header byte. Every byte value is a valid header.
    pub(crate) const fn from_byte(byte: u8) -> Self {
        if byte & Self::COMPRESSED != 0 {
            Self::CompressedTimestamp {
                local: (byte >> Self::COMPRESSED_LOCAL_SHIFT) & Self::COMPRESSED_LOCAL_MASK,
                time_offset: byte & Self::TIME_OFFSET_MASK,
            }
        } else if byte & Self::DEFINITION != 0 {
            Self::Definition {
                local: byte & Self::LOCAL_MASK,
                has_developer_fields: byte & Self::DEVELOPER_DATA != 0,
            }
        } else {
            Self::Data {
                local: byte & Self::LOCAL_MASK,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_data() {
        assert_eq!(
            RecordHeader::from_byte(0x00),
            RecordHeader::Data { local: 0 }
        );
        assert_eq!(
            RecordHeader::from_byte(0x0F),
            RecordHeader::Data { local: 15 }
        );
        // Reserved bit 4 and the developer flag are ignored on data messages.
        assert_eq!(
            RecordHeader::from_byte(0x33),
            RecordHeader::Data { local: 3 }
        );
    }

    #[test]
    fn definition() {
        assert_eq!(
            RecordHeader::from_byte(0x40),
            RecordHeader::Definition {
                local: 0,
                has_developer_fields: false
            }
        );
        assert_eq!(
            RecordHeader::from_byte(0x6F),
            RecordHeader::Definition {
                local: 15,
                has_developer_fields: true
            }
        );
        assert_eq!(
            RecordHeader::from_byte(0x52),
            RecordHeader::Definition {
                local: 2,
                has_developer_fields: false
            }
        );
    }

    #[test]
    fn compressed_timestamp() {
        assert_eq!(
            RecordHeader::from_byte(0x80),
            RecordHeader::CompressedTimestamp {
                local: 0,
                time_offset: 0
            }
        );
        assert_eq!(
            RecordHeader::from_byte(0xFF),
            RecordHeader::CompressedTimestamp {
                local: 3,
                time_offset: 31
            }
        );
        assert_eq!(
            RecordHeader::from_byte(0b1010_0101),
            RecordHeader::CompressedTimestamp {
                local: 1,
                time_offset: 5
            }
        );
    }

    #[test]
    fn every_byte_decodes_consistently() {
        for b in 0..=255u8 {
            match RecordHeader::from_byte(b) {
                RecordHeader::CompressedTimestamp { local, time_offset } => {
                    assert!(b >= 0x80);
                    assert!(local < 4 && time_offset < 32);
                    assert_eq!(0x80 | (local << 5) | time_offset, b);
                }
                RecordHeader::Definition { local, .. } => {
                    assert_eq!(b & 0xC0, 0x40);
                    assert!(local < 16);
                }
                RecordHeader::Data { local } => {
                    assert_eq!(b & 0xC0, 0x00);
                    assert!(local < 16);
                }
            }
        }
    }
}
