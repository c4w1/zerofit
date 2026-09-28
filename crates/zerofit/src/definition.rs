//! Definition messages.
//!
//! A definition message describes the layout of the data messages that follow
//! it for one local message type:
//!
//! | Offset    | Size | Content                                              |
//! |----------:|-----:|------------------------------------------------------|
//! | 0         | 1    | Reserved                                             |
//! | 1         | 1    | Architecture: 0 little-endian, 1 big-endian          |
//! | 2         | 2    | Global message number, in the architecture's order   |
//! | 4         | 1    | Number of fields, `n`                                |
//! | 5         | 3n   | Field definitions: number, size, base type           |
//! | 5+3n      | 1    | Number of developer fields, `m` (if flagged)         |
//! | 6+3n      | 3m   | Developer field definitions: number, size, dev index |

use core::slice::ChunksExact;

use crate::base_type::{BaseType, Endian};
use crate::error::{Error, ErrorKind};

/// Field number used for `timestamp` in every FIT message.
pub(crate) const TIMESTAMP_FIELD: u8 = 253;

/// Size of the fixed part of a definition message, after the record header.
const FIXED_SIZE: usize = 5;

/// Bytes per field definition.
const FIELD_DEF_SIZE: usize = 3;

/// A parsed definition message, borrowing its field list from the input.
///
/// `Definition` is `Copy`: it is a handful of integers and two slices, so the
/// decoder can hand out copies without allocating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Definition<'a> {
    local_message_type: u8,
    endian: Endian,
    global_message_number: u16,
    fields: &'a [u8],
    developer_fields: &'a [u8],
    message_size: usize,
    timestamp_offset: Option<usize>,
}

impl<'a> Definition<'a> {
    /// Parses a definition message body (the bytes after the record header).
    ///
    /// Returns the definition and the number of body bytes consumed. Errors
    /// are reported at `record_offset`, the absolute offset of the record
    /// header byte.
    pub(crate) fn parse(
        body: &'a [u8],
        local_message_type: u8,
        has_developer_fields: bool,
        record_offset: u64,
    ) -> Result<(Self, usize), Error> {
        let err = |kind| Error::new(kind, record_offset);
        let eof = |needed: usize, have: usize| {
            err(ErrorKind::UnexpectedEof {
                needed: needed.saturating_sub(have),
            })
        };

        let Some((&[_reserved, arch, g0, g1, field_count], rest)) =
            body.split_first_chunk::<FIXED_SIZE>()
        else {
            return Err(eof(FIXED_SIZE, body.len()));
        };
        let endian =
            Endian::from_architecture(arch).ok_or(err(ErrorKind::InvalidArchitecture(arch)))?;
        let global_message_number = match endian {
            Endian::Little => u16::from_le_bytes([g0, g1]),
            Endian::Big => u16::from_be_bytes([g0, g1]),
        };

        let fields_len = usize::from(field_count).saturating_mul(FIELD_DEF_SIZE);
        let Some((fields, rest)) = rest.split_at_checked(fields_len) else {
            return Err(eof(fields_len, rest.len()));
        };
        let mut consumed = FIXED_SIZE.saturating_add(fields_len);

        let developer_fields: &'a [u8] = if has_developer_fields {
            let Some((&dev_count, rest)) = rest.split_first() else {
                return Err(eof(1, 0));
            };
            let dev_len = usize::from(dev_count).saturating_mul(FIELD_DEF_SIZE);
            let Some((dev, _)) = rest.split_at_checked(dev_len) else {
                return Err(eof(dev_len, rest.len()));
            };
            consumed = consumed.saturating_add(1).saturating_add(dev_len);
            dev
        } else {
            &[]
        };

        // Precompute the data message size and where the timestamp lives.
        let mut message_size: usize = 0;
        let mut timestamp_offset = None;
        for def in fields.chunks_exact(FIELD_DEF_SIZE) {
            if let &[number, size, _] = def {
                if number == TIMESTAMP_FIELD && size == 4 && timestamp_offset.is_none() {
                    timestamp_offset = Some(message_size);
                }
                message_size = message_size.saturating_add(usize::from(size));
            }
        }
        for def in developer_fields.chunks_exact(FIELD_DEF_SIZE) {
            if let &[_, size, _] = def {
                message_size = message_size.saturating_add(usize::from(size));
            }
        }

        Ok((
            Self {
                local_message_type,
                endian,
                global_message_number,
                fields,
                developer_fields,
                message_size,
                timestamp_offset,
            },
            consumed,
        ))
    }

    /// Local message type (0-15) this definition is bound to.
    #[must_use]
    pub const fn local_message_type(&self) -> u8 {
        self.local_message_type
    }

    /// Byte order of multi-byte values in data messages using this definition.
    #[must_use]
    pub const fn endian(&self) -> Endian {
        self.endian
    }

    /// Global message number from the FIT profile (for example 20 for
    /// `record`).
    #[must_use]
    pub const fn global_message_number(&self) -> u16 {
        self.global_message_number
    }

    /// Size in bytes of each data message using this definition, excluding the
    /// record header byte.
    #[must_use]
    pub const fn message_size(&self) -> usize {
        self.message_size
    }

    /// Number of regular fields.
    #[must_use]
    pub const fn field_count(&self) -> usize {
        self.fields.len() / FIELD_DEF_SIZE
    }

    /// Number of developer fields.
    #[must_use]
    pub const fn developer_field_count(&self) -> usize {
        self.developer_fields.len() / FIELD_DEF_SIZE
    }

    /// Regular field definitions, in data message order.
    #[must_use]
    pub fn fields(&self) -> FieldDefinitions<'a> {
        FieldDefinitions(self.fields.chunks_exact(FIELD_DEF_SIZE))
    }

    /// Developer field definitions, in data message order. They follow the
    /// regular fields in each data message.
    #[must_use]
    pub fn developer_fields(&self) -> DeveloperFieldDefinitions<'a> {
        DeveloperFieldDefinitions(self.developer_fields.chunks_exact(FIELD_DEF_SIZE))
    }

    /// Byte offset of a 4-byte field 253 (`timestamp`) within each data
    /// message, if the definition has one.
    pub(crate) const fn timestamp_offset(&self) -> Option<usize> {
        self.timestamp_offset
    }

    /// Total size of the regular fields in each data message.
    pub(crate) fn regular_fields_size(&self) -> usize {
        self.fields().map(|f| usize::from(f.size())).sum()
    }
}

/// One regular field definition: which field, how many bytes, what type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldDefinition {
    number: u8,
    size: u8,
    base_type_byte: u8,
}

impl FieldDefinition {
    /// Field definition number from the FIT profile.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.number
    }

    /// Size of the field in bytes.
    #[must_use]
    pub const fn size(&self) -> u8 {
        self.size
    }

    /// The base type byte exactly as stored in the definition.
    #[must_use]
    pub const fn base_type_byte(&self) -> u8 {
        self.base_type_byte
    }

    /// The field's base type. Unknown base type numbers are reported as
    /// [`BaseType::Byte`], so their data is still available as raw bytes.
    #[must_use]
    pub const fn base_type(&self) -> BaseType {
        match BaseType::from_byte(self.base_type_byte) {
            Some(t) => t,
            None => BaseType::Byte,
        }
    }
}

/// One developer field definition. Its meaning is given by a
/// `field_description` message elsewhere in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeveloperFieldDefinition {
    number: u8,
    size: u8,
    developer_data_index: u8,
}

impl DeveloperFieldDefinition {
    /// Developer field number, matching `field_description.field_definition_number`.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.number
    }

    /// Size of the field in bytes.
    #[must_use]
    pub const fn size(&self) -> u8 {
        self.size
    }

    /// Developer data index, matching `developer_data_id.developer_data_index`.
    #[must_use]
    pub const fn developer_data_index(&self) -> u8 {
        self.developer_data_index
    }
}

/// Iterator over a definition's [`FieldDefinition`]s.
#[derive(Debug, Clone)]
pub struct FieldDefinitions<'a>(ChunksExact<'a, u8>);

impl Iterator for FieldDefinitions<'_> {
    type Item = FieldDefinition;

    fn next(&mut self) -> Option<Self::Item> {
        match *self.0.next()? {
            [number, size, base_type_byte] => Some(FieldDefinition {
                number,
                size,
                base_type_byte,
            }),
            _ => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for FieldDefinitions<'_> {}

/// Iterator over a definition's [`DeveloperFieldDefinition`]s.
#[derive(Debug, Clone)]
pub struct DeveloperFieldDefinitions<'a>(ChunksExact<'a, u8>);

impl Iterator for DeveloperFieldDefinitions<'_> {
    type Item = DeveloperFieldDefinition;

    fn next(&mut self) -> Option<Self::Item> {
        match *self.0.next()? {
            [number, size, developer_data_index] => Some(DeveloperFieldDefinition {
                number,
                size,
                developer_data_index,
            }),
            _ => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for DeveloperFieldDefinitions<'_> {}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn body(arch: u8, global: [u8; 2], fields: &[[u8; 3]], dev: Option<&[[u8; 3]]>) -> Vec<u8> {
        let mut b = std::vec![0, arch, global[0], global[1], fields.len() as u8];
        b.extend(fields.iter().flatten());
        if let Some(dev) = dev {
            b.push(dev.len() as u8);
            b.extend(dev.iter().flatten());
        }
        b
    }

    #[test]
    fn little_endian_definition() {
        let b = body(
            0,
            20u16.to_le_bytes(),
            &[[253, 4, 0x86], [3, 1, 0x02], [5, 4, 0x86]],
            None,
        );
        let (def, used) = Definition::parse(&b, 2, false, 100).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(def.local_message_type(), 2);
        assert_eq!(def.endian(), Endian::Little);
        assert_eq!(def.global_message_number(), 20);
        assert_eq!(def.field_count(), 3);
        assert_eq!(def.developer_field_count(), 0);
        assert_eq!(def.message_size(), 9);
        assert_eq!(def.timestamp_offset(), Some(0));
        let f: Vec<_> = def.fields().collect();
        assert_eq!(f[1].number(), 3);
        assert_eq!(f[1].size(), 1);
        assert_eq!(f[1].base_type(), BaseType::UInt8);
        assert_eq!(f[2].base_type_byte(), 0x86);
        assert_eq!(def.fields().len(), 3);
    }

    #[test]
    fn big_endian_global_number() {
        let b = body(1, 0x0102u16.to_be_bytes(), &[[0, 1, 0]], None);
        let (def, _) = Definition::parse(&b, 0, false, 0).unwrap();
        assert_eq!(def.endian(), Endian::Big);
        assert_eq!(def.global_message_number(), 0x0102);
    }

    #[test]
    fn developer_fields() {
        let b = body(0, [20, 0], &[[3, 1, 2]], Some(&[[0, 2, 0], [1, 4, 1]]));
        let (def, used) = Definition::parse(&b, 0, true, 0).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(def.developer_field_count(), 2);
        assert_eq!(def.message_size(), 1 + 2 + 4);
        assert_eq!(def.regular_fields_size(), 1);
        let d: Vec<_> = def.developer_fields().collect();
        assert_eq!(d[1].number(), 1);
        assert_eq!(d[1].size(), 4);
        assert_eq!(d[1].developer_data_index(), 1);
    }

    #[test]
    fn zero_developer_fields() {
        let b = body(0, [0, 0], &[], Some(&[]));
        let (def, used) = Definition::parse(&b, 0, true, 0).unwrap();
        assert_eq!(used, 6);
        assert_eq!(def.message_size(), 0);
    }

    #[test]
    fn empty_definition() {
        let b = body(0, [0, 0], &[], None);
        let (def, used) = Definition::parse(&b, 15, false, 0).unwrap();
        assert_eq!(used, 5);
        assert_eq!(def.field_count(), 0);
        assert_eq!(def.message_size(), 0);
        assert_eq!(def.timestamp_offset(), None);
    }

    #[test]
    fn timestamp_offset_requires_four_bytes() {
        let b = body(
            0,
            [0, 0],
            &[[1, 2, 0x84], [253, 2, 0x84], [253, 4, 0x86]],
            None,
        );
        let (def, _) = Definition::parse(&b, 0, false, 0).unwrap();
        assert_eq!(def.timestamp_offset(), Some(4));
    }

    #[test]
    fn unknown_base_type_reads_as_bytes() {
        let b = body(0, [0, 0], &[[1, 2, 0x1F]], None);
        let (def, _) = Definition::parse(&b, 0, false, 0).unwrap();
        let f = def.fields().next().unwrap();
        assert_eq!(f.base_type(), BaseType::Byte);
        assert_eq!(f.base_type_byte(), 0x1F);
    }

    #[test]
    fn invalid_architecture() {
        let b = body(2, [0, 0], &[], None);
        let e = Definition::parse(&b, 0, false, 77).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::InvalidArchitecture(2));
        assert_eq!(e.offset(), 77);
    }

    #[test]
    fn every_truncation_is_eof() {
        let b = body(0, [20, 0], &[[3, 1, 2], [4, 2, 0x84]], Some(&[[0, 1, 0]]));
        for len in 0..b.len() {
            let e = Definition::parse(&b[..len], 0, true, 9).unwrap_err();
            assert!(
                matches!(e.kind(), ErrorKind::UnexpectedEof { needed } if needed > 0),
                "len {len}: {e:?}"
            );
            assert_eq!(e.offset(), 9);
        }
    }

    #[test]
    fn trailing_bytes_are_not_consumed() {
        let mut b = body(0, [0, 0], &[[1, 1, 2]], None);
        b.extend_from_slice(&[0xAA, 0xBB]);
        let (_, used) = Definition::parse(&b, 0, false, 0).unwrap();
        assert_eq!(used, 8);
    }

    #[test]
    fn maximum_sizes() {
        let fields: Vec<[u8; 3]> = (0..=254).map(|n| [n, 255, 0x0D]).collect();
        let b = body(0, [0, 0], &fields, Some(&fields));
        let (def, used) = Definition::parse(&b, 0, true, 0).unwrap();
        assert_eq!(used, b.len());
        assert_eq!(def.message_size(), 2 * 255 * 255);
    }
}
