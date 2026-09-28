//! Data messages and their fields.

use crate::base_type::{BaseType, Endian};
use crate::definition::{
    Definition, DeveloperFieldDefinition, DeveloperFieldDefinitions, FieldDefinition,
    FieldDefinitions,
};
use crate::value::Value;

/// A data message: the bytes of one record plus the definition that describes
/// them. Borrows from the decoder's input; field values are decoded lazily.
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// use zerofit::{Decoder, Record, Value};
///
/// for record in Decoder::new(bytes) {
///     if let Record::Data(msg) = record? {
///         assert_eq!(msg.global_message_number(), 20); // `record`
///         assert_eq!(msg.timestamp(), Some(1_000_000_000));
///         let heart_rate = msg.field(3).and_then(|f| f.value());
///         assert_eq!(heart_rate, Some(Value::UInt8(142)));
///     }
/// }
/// # Ok::<(), zerofit::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataMessage<'a> {
    definition: Definition<'a>,
    bytes: &'a [u8],
    timestamp: Option<u32>,
    offset: u64,
    compressed_timestamp: bool,
}

impl<'a> DataMessage<'a> {
    pub(crate) const fn new(
        definition: Definition<'a>,
        bytes: &'a [u8],
        timestamp: Option<u32>,
        offset: u64,
        compressed_timestamp: bool,
    ) -> Self {
        Self {
            definition,
            bytes,
            timestamp,
            offset,
            compressed_timestamp,
        }
    }

    /// Global message number from the FIT profile (for example 20 for
    /// `record`).
    #[must_use]
    pub const fn global_message_number(&self) -> u16 {
        self.definition.global_message_number()
    }

    /// Local message type this message was written with.
    #[must_use]
    pub const fn local_message_type(&self) -> u8 {
        self.definition.local_message_type()
    }

    /// The definition describing this message's layout.
    #[must_use]
    pub const fn definition(&self) -> &Definition<'a> {
        &self.definition
    }

    /// The message's timestamp in seconds since the FIT epoch
    /// (1989-12-31T00:00:00Z).
    ///
    /// Taken from field 253 if present and valid, otherwise computed from a
    /// compressed timestamp header and the most recent full timestamp.
    /// `None` if neither is available.
    #[must_use]
    pub const fn timestamp(&self) -> Option<u32> {
        self.timestamp
    }

    /// Whether this message used a compressed timestamp header.
    #[must_use]
    pub const fn has_compressed_timestamp(&self) -> bool {
        self.compressed_timestamp
    }

    /// Absolute byte offset of this message's record header in the input.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// The message's bytes (all fields, excluding the record header byte).
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Iterates over the regular fields in definition order.
    #[must_use]
    pub fn fields(&self) -> Fields<'a> {
        Fields {
            defs: self.definition.fields(),
            endian: self.definition.endian(),
            rest: self.bytes,
        }
    }

    /// The first regular field with definition number `number`.
    #[must_use]
    pub fn field(&self, number: u8) -> Option<Field<'a>> {
        self.fields().find(|f| f.number() == number)
    }

    /// Iterates over the developer fields, which follow the regular fields.
    #[must_use]
    pub fn developer_fields(&self) -> DeveloperFields<'a> {
        let regular = self.definition.regular_fields_size();
        DeveloperFields {
            defs: self.definition.developer_fields(),
            rest: self.bytes.get(regular..).unwrap_or(&[]),
        }
    }
}

/// One regular field of a [`DataMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<'a> {
    definition: FieldDefinition,
    endian: Endian,
    bytes: &'a [u8],
}

impl<'a> Field<'a> {
    /// Field definition number from the FIT profile.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.definition.number()
    }

    /// The field's base type (see [`FieldDefinition::base_type`]).
    #[must_use]
    pub const fn base_type(&self) -> BaseType {
        self.definition.base_type()
    }

    /// The field's definition.
    #[must_use]
    pub const fn definition(&self) -> FieldDefinition {
        self.definition
    }

    /// The field's raw bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The decoded value, or `None` if it is the base type's invalid
    /// sentinel (the device had no value).
    #[must_use]
    pub fn value(&self) -> Option<Value<'a>> {
        Some(self.raw_value()).filter(|v| !v.is_invalid())
    }

    /// The decoded value, including invalid sentinels.
    #[must_use]
    pub fn raw_value(&self) -> Value<'a> {
        Value::decode(self.base_type(), self.endian, self.bytes)
    }
}

/// One developer field of a [`DataMessage`], as raw bytes. Interpreting it
/// requires the matching `field_description` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeveloperField<'a> {
    definition: DeveloperFieldDefinition,
    bytes: &'a [u8],
}

impl<'a> DeveloperField<'a> {
    /// Developer field number.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.definition.number()
    }

    /// Developer data index identifying the application that defined it.
    #[must_use]
    pub const fn developer_data_index(&self) -> u8 {
        self.definition.developer_data_index()
    }

    /// The field's definition.
    #[must_use]
    pub const fn definition(&self) -> DeveloperFieldDefinition {
        self.definition
    }

    /// The field's raw bytes.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Iterator over the regular fields of a [`DataMessage`].
#[derive(Debug, Clone)]
pub struct Fields<'a> {
    defs: FieldDefinitions<'a>,
    endian: Endian,
    rest: &'a [u8],
}

impl<'a> Iterator for Fields<'a> {
    type Item = Field<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let definition = self.defs.next()?;
        let (bytes, rest) = self.rest.split_at_checked(usize::from(definition.size()))?;
        self.rest = rest;
        Some(Field {
            definition,
            endian: self.endian,
            bytes,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.defs.size_hint().1)
    }
}

/// Iterator over the developer fields of a [`DataMessage`].
#[derive(Debug, Clone)]
pub struct DeveloperFields<'a> {
    defs: DeveloperFieldDefinitions<'a>,
    rest: &'a [u8],
}

impl<'a> Iterator for DeveloperFields<'a> {
    type Item = DeveloperField<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let definition = self.defs.next()?;
        let (bytes, rest) = self.rest.split_at_checked(usize::from(definition.size()))?;
        self.rest = rest;
        Some(DeveloperField { definition, bytes })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.defs.size_hint().1)
    }
}
