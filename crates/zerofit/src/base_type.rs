//! FIT base types and byte order.

/// One of the 17 FIT base types that every field is encoded as.
///
/// In a definition message the base type is one byte: the low 5 bits hold the
/// base type number and bit 7 is set for multi-byte types ("endian ability").
/// Decoders identify the type by the number alone, as the FIT SDK does.
///
/// | Type      | Number | Byte   | Size | Invalid value           |
/// |-----------|-------:|--------|-----:|-------------------------|
/// | `Enum`    | 0      | `0x00` | 1    | `0xFF`                  |
/// | `SInt8`   | 1      | `0x01` | 1    | `0x7F`                  |
/// | `UInt8`   | 2      | `0x02` | 1    | `0xFF`                  |
/// | `SInt16`  | 3      | `0x83` | 2    | `0x7FFF`                |
/// | `UInt16`  | 4      | `0x84` | 2    | `0xFFFF`                |
/// | `SInt32`  | 5      | `0x85` | 4    | `0x7FFF_FFFF`           |
/// | `UInt32`  | 6      | `0x86` | 4    | `0xFFFF_FFFF`           |
/// | `String`  | 7      | `0x07` | 1    | empty (first byte NUL)  |
/// | `Float32` | 8      | `0x88` | 4    | all bits set            |
/// | `Float64` | 9      | `0x89` | 8    | all bits set            |
/// | `UInt8z`  | 10     | `0x0A` | 1    | `0x00`                  |
/// | `UInt16z` | 11     | `0x8B` | 2    | `0x0000`                |
/// | `UInt32z` | 12     | `0x8C` | 4    | `0x0000_0000`           |
/// | `Byte`    | 13     | `0x0D` | 1    | every byte `0xFF`       |
/// | `SInt64`  | 14     | `0x8E` | 8    | `0x7FFF_FFFF_FFFF_FFFF` |
/// | `UInt64`  | 15     | `0x8F` | 8    | `0xFFFF_FFFF_FFFF_FFFF` |
/// | `UInt64z` | 16     | `0x90` | 8    | `0`                     |
///
/// ```
/// use zerofit::BaseType;
///
/// assert_eq!(BaseType::from_byte(0x84), Some(BaseType::UInt16));
/// // The endian-ability bit is ignored when identifying the type.
/// assert_eq!(BaseType::from_byte(0x04), Some(BaseType::UInt16));
/// assert_eq!(BaseType::UInt16.size(), 2);
/// assert_eq!(BaseType::from_byte(0x1F), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BaseType {
    /// Enumeration stored as an unsigned byte.
    Enum = 0,
    /// Signed 8-bit integer.
    SInt8 = 1,
    /// Unsigned 8-bit integer.
    UInt8 = 2,
    /// Signed 16-bit integer.
    SInt16 = 3,
    /// Unsigned 16-bit integer.
    UInt16 = 4,
    /// Signed 32-bit integer.
    SInt32 = 5,
    /// Unsigned 32-bit integer.
    UInt32 = 6,
    /// NUL-terminated UTF-8 string.
    String = 7,
    /// IEEE 754 single-precision float.
    Float32 = 8,
    /// IEEE 754 double-precision float.
    Float64 = 9,
    /// Unsigned 8-bit integer whose invalid value is zero.
    UInt8z = 10,
    /// Unsigned 16-bit integer whose invalid value is zero.
    UInt16z = 11,
    /// Unsigned 32-bit integer whose invalid value is zero.
    UInt32z = 12,
    /// Opaque byte array.
    Byte = 13,
    /// Signed 64-bit integer.
    SInt64 = 14,
    /// Unsigned 64-bit integer.
    UInt64 = 15,
    /// Unsigned 64-bit integer whose invalid value is zero.
    UInt64z = 16,
}

impl BaseType {
    /// Mask selecting the base type number from a base type byte.
    const NUMBER_MASK: u8 = 0x1F;

    /// All base types, in number order.
    pub const ALL: [Self; 17] = [
        Self::Enum,
        Self::SInt8,
        Self::UInt8,
        Self::SInt16,
        Self::UInt16,
        Self::SInt32,
        Self::UInt32,
        Self::String,
        Self::Float32,
        Self::Float64,
        Self::UInt8z,
        Self::UInt16z,
        Self::UInt32z,
        Self::Byte,
        Self::SInt64,
        Self::UInt64,
        Self::UInt64z,
    ];

    /// Identifies a base type from a definition message's base type byte,
    /// using only its low 5 bits. Returns `None` for unknown numbers.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte & Self::NUMBER_MASK {
            0 => Self::Enum,
            1 => Self::SInt8,
            2 => Self::UInt8,
            3 => Self::SInt16,
            4 => Self::UInt16,
            5 => Self::SInt32,
            6 => Self::UInt32,
            7 => Self::String,
            8 => Self::Float32,
            9 => Self::Float64,
            10 => Self::UInt8z,
            11 => Self::UInt16z,
            12 => Self::UInt32z,
            13 => Self::Byte,
            14 => Self::SInt64,
            15 => Self::UInt64,
            16 => Self::UInt64z,
            _ => return None,
        })
    }

    /// The base type number (0 to 16).
    #[must_use]
    pub const fn number(self) -> u8 {
        self as u8
    }

    /// The canonical base type byte, with the endian-ability bit set for
    /// multi-byte types.
    ///
    /// ```
    /// assert_eq!(zerofit::BaseType::UInt32.to_byte(), 0x86);
    /// assert_eq!(zerofit::BaseType::UInt8.to_byte(), 0x02);
    /// ```
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        if self.size() > 1 {
            self.number() | 0x80
        } else {
            self.number()
        }
    }

    /// Size in bytes of one element of this type.
    #[must_use]
    pub const fn size(self) -> usize {
        match self {
            Self::Enum | Self::SInt8 | Self::UInt8 | Self::String | Self::UInt8z | Self::Byte => 1,
            Self::SInt16 | Self::UInt16 | Self::UInt16z => 2,
            Self::SInt32 | Self::UInt32 | Self::Float32 | Self::UInt32z => 4,
            Self::Float64 | Self::SInt64 | Self::UInt64 | Self::UInt64z => 8,
        }
    }
}

/// Byte order of multi-byte field values, set per definition message by its
/// architecture byte.
///
/// ```
/// use zerofit::Endian;
///
/// assert_eq!(Endian::from_architecture(0), Some(Endian::Little));
/// assert_eq!(Endian::from_architecture(1), Some(Endian::Big));
/// assert_eq!(Endian::from_architecture(2), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Endian {
    /// Least significant byte first (architecture byte 0).
    #[default]
    Little,
    /// Most significant byte first (architecture byte 1).
    Big,
}

impl Endian {
    /// Maps a definition message's architecture byte to a byte order.
    #[must_use]
    pub const fn from_architecture(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Little),
            1 => Some(Self::Big),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_round_trip_for_all_types() {
        for (i, t) in BaseType::ALL.iter().enumerate() {
            assert_eq!(usize::from(t.number()), i);
            assert_eq!(BaseType::from_byte(t.to_byte()), Some(*t));
            assert_eq!(BaseType::from_byte(t.number()), Some(*t));
            assert_eq!(BaseType::from_byte(t.number() | 0x80), Some(*t));
        }
    }

    #[test]
    fn canonical_bytes_match_spec() {
        let expected = [
            0x00, 0x01, 0x02, 0x83, 0x84, 0x85, 0x86, 0x07, 0x88, 0x89, 0x0A, 0x8B, 0x8C, 0x0D,
            0x8E, 0x8F, 0x90,
        ];
        for (t, byte) in BaseType::ALL.iter().zip(expected) {
            assert_eq!(t.to_byte(), byte, "{t:?}");
        }
    }

    #[test]
    fn unknown_numbers_are_rejected() {
        for n in 17..=0x1F {
            assert_eq!(BaseType::from_byte(n), None);
            assert_eq!(BaseType::from_byte(n | 0x80), None);
        }
    }

    #[test]
    fn architecture() {
        assert_eq!(Endian::from_architecture(0), Some(Endian::Little));
        assert_eq!(Endian::from_architecture(1), Some(Endian::Big));
        for b in 2..=255 {
            assert_eq!(Endian::from_architecture(b), None);
        }
    }
}
