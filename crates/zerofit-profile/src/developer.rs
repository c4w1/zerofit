//! Developer field resolution (requires the `alloc` feature).
//!
//! Developer fields are fields that an app (a Connect IQ data field, a
//! power meter's companion app) adds to standard messages. A data message
//! only carries their bytes, tagged with a developer data index and a field
//! number; what they mean is declared earlier in the same file by a
//! `field_description` message, and the app by a `developer_data_id`
//! message. [`DeveloperData`] collects those declarations as the file is
//! decoded and resolves developer fields against them.

use alloc::collections::BTreeMap;
use alloc::string::String;

use zerofit::{BaseType, DataMessage, DeveloperField, Value};

/// `field_description` (profile message 206) and the fields read here.
const FIELD_DESCRIPTION: u16 = 206;
const FD_DEVELOPER_DATA_INDEX: u8 = 0;
const FD_FIELD_DEFINITION_NUMBER: u8 = 1;
const FD_FIT_BASE_TYPE_ID: u8 = 2;
const FD_FIELD_NAME: u8 = 3;
const FD_SCALE: u8 = 6;
const FD_OFFSET: u8 = 7;
const FD_UNITS: u8 = 8;
const FD_NATIVE_MESG_NUM: u8 = 14;
const FD_NATIVE_FIELD_NUM: u8 = 15;

/// `developer_data_id` (profile message 207) and the fields read here.
const DEVELOPER_DATA_ID: u16 = 207;
const DD_APPLICATION_ID: u8 = 1;
const DD_DEVELOPER_DATA_INDEX: u8 = 3;
const DD_APPLICATION_VERSION: u8 = 4;

/// What a `field_description` message declares about one developer field.
///
/// Obtained from [`DeveloperData::description`] or
/// [`ResolvedField::description`]; see [`ResolvedField`] for an example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    developer_data_index: u8,
    field_number: u8,
    base_type: BaseType,
    name: Option<String>,
    units: Option<String>,
    scale: Option<u8>,
    offset: Option<i8>,
    native_message: Option<u16>,
    native_field: Option<u8>,
}

impl FieldDescription {
    /// Developer data index of the app that declared the field.
    #[must_use]
    pub const fn developer_data_index(&self) -> u8 {
        self.developer_data_index
    }

    /// Developer field number.
    #[must_use]
    pub const fn field_number(&self) -> u8 {
        self.field_number
    }

    /// Base type of the field's values.
    #[must_use]
    pub const fn base_type(&self) -> BaseType {
        self.base_type
    }

    /// Field name, e.g. `Glucose`.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Units, e.g. `mg/dL`.
    #[must_use]
    pub fn units(&self) -> Option<&str> {
        self.units.as_deref()
    }

    /// Scale, if the app declared one. The value is `raw / scale - offset`.
    #[must_use]
    pub const fn scale(&self) -> Option<u8> {
        self.scale
    }

    /// Offset, if the app declared one.
    #[must_use]
    pub const fn offset(&self) -> Option<i8> {
        self.offset
    }

    /// The standard message this field duplicates, if any (for example a
    /// heart-rate field that mirrors `record.heart_rate`).
    #[must_use]
    pub const fn native_message(&self) -> Option<u16> {
        self.native_message
    }

    /// The standard field this field duplicates, if any.
    #[must_use]
    pub const fn native_field(&self) -> Option<u8> {
        self.native_field
    }
}

/// An app that registered developer fields (`developer_data_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Application {
    /// 16-byte application UUID, if given.
    pub application_id: Option<[u8; 16]>,
    /// Application version, if given.
    pub application_version: Option<u32>,
}

/// A developer field together with its description and decoded value.
///
/// Produced by [`DeveloperData::resolve`]:
///
/// ```
/// use zerofit::encode::{Encoder, FileOptions};
/// use zerofit::{DeveloperFieldDefinition, Decoder, Endian, FieldDefinition, Record};
/// use zerofit_profile::developer::DeveloperData;
///
/// let mut enc = Encoder::new();
/// enc.begin_file(FileOptions::new(2132))?;
/// // field_description: app 0, field 1, uint8, named "Wind", scale 10, units "m/s".
/// enc.write_definition(0, Endian::Little, 206, &[
///     FieldDefinition::new(0, 1, 0x02), FieldDefinition::new(1, 1, 0x02),
///     FieldDefinition::new(2, 1, 0x02), FieldDefinition::new(3, 5, 0x07),
///     FieldDefinition::new(6, 1, 0x02), FieldDefinition::new(8, 4, 0x07),
/// ], &[])?;
/// enc.write_data(0, &[0, 1, 0x02, b'W', b'i', b'n', b'd', 0, 10, b'm', b'/', b's', 0])?;
/// // A record carrying that developer field.
/// enc.write_definition(1, Endian::Little, 20, &[], &[DeveloperFieldDefinition::new(1, 1, 0)])?;
/// enc.write_data(1, &[42])?;
/// let bytes = enc.finish()?;
///
/// let mut developer = DeveloperData::new();
/// for record in Decoder::new(&bytes) {
///     if let Ok(Record::Data(msg)) = record {
///         developer.observe(&msg);
///         for field in developer.resolve(&msg) {
///             assert_eq!(field.description.name(), Some("Wind"));
///             assert_eq!(field.scaled(), Some(4.2));
///             assert_eq!(field.description.units(), Some("m/s"));
///         }
///     }
/// }
/// # Ok::<(), zerofit::encode::EncodeError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedField<'a, 'd> {
    /// The field's declaration.
    pub description: &'d FieldDescription,
    /// The raw field.
    pub field: DeveloperField<'a>,
}

impl<'a> ResolvedField<'a, '_> {
    /// The value decoded with the declared base type and the message's byte
    /// order, or `None` if it is the invalid sentinel.
    #[must_use]
    pub fn value(&self) -> Option<Value<'a>> {
        Some(self.raw_value()).filter(|v| !v.is_invalid())
    }

    /// The value including invalid sentinels.
    #[must_use]
    pub fn raw_value(&self) -> Value<'a> {
        Value::decode(
            self.description.base_type,
            self.field.endian(),
            self.field.bytes(),
        )
    }

    /// A numeric value with the declared scale and offset applied.
    #[must_use]
    pub fn scaled(&self) -> Option<f64> {
        let v = self.value()?.as_f64()?;
        let scale = self.description.scale.map_or(1.0, f64::from);
        let offset = self.description.offset.map_or(0.0, f64::from);
        Some(v / scale - offset)
    }
}

/// Developer field declarations seen so far in a file.
///
/// Feed every data message to [`observe`](Self::observe) in file order (it
/// ignores messages it does not need), then call
/// [`resolve`](Self::resolve) on the messages whose developer fields you
/// want. Call [`clear`](Self::clear) when a new file starts in a chained
/// stream.
///
/// ```
/// use zerofit::{Decoder, Record};
/// use zerofit_profile::developer::DeveloperData;
///
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// let mut developer = DeveloperData::new();
/// for record in Decoder::new(bytes) {
///     match record? {
///         Record::Header(_) => developer.clear(),
///         Record::Data(msg) => {
///             developer.observe(&msg);
///             for field in developer.resolve(&msg) {
///                 println!("{:?} = {:?} {:?}",
///                     field.description.name(), field.scaled(), field.description.units());
///             }
///         }
///         _ => {}
///     }
/// }
/// # Ok::<(), zerofit::Error>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct DeveloperData {
    fields: BTreeMap<(u8, u8), FieldDescription>,
    applications: BTreeMap<u8, Application>,
}

impl DeveloperData {
    /// An empty set of declarations.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets all declarations (call at the start of each file).
    pub fn clear(&mut self) {
        self.fields.clear();
        self.applications.clear();
    }

    /// Learns from `msg` if it is a `field_description` or
    /// `developer_data_id` message. Returns whether it was one.
    pub fn observe(&mut self, msg: &DataMessage<'_>) -> bool {
        match msg.global_message_number() {
            FIELD_DESCRIPTION => {
                if let Some(d) = parse_description(msg) {
                    self.fields
                        .insert((d.developer_data_index, d.field_number), d);
                }
                true
            }
            DEVELOPER_DATA_ID => {
                if let Some(index) = uint(msg, DD_DEVELOPER_DATA_INDEX) {
                    let application_id = msg
                        .field(DD_APPLICATION_ID)
                        .filter(|f| f.value().is_some())
                        .and_then(|f| f.bytes().first_chunk::<16>().copied());
                    self.applications.insert(
                        index,
                        Application {
                            application_id,
                            application_version: uint(msg, DD_APPLICATION_VERSION),
                        },
                    );
                }
                true
            }
            _ => false,
        }
    }

    /// The declaration of developer field `field_number` of app
    /// `developer_data_index`.
    #[must_use]
    pub fn description(
        &self,
        developer_data_index: u8,
        field_number: u8,
    ) -> Option<&FieldDescription> {
        self.fields.get(&(developer_data_index, field_number))
    }

    /// The app registered under `developer_data_index`.
    #[must_use]
    pub fn application(&self, developer_data_index: u8) -> Option<&Application> {
        self.applications.get(&developer_data_index)
    }

    /// The developer fields of `msg` that have a declaration. Fields without
    /// one are skipped (their bytes stay available through
    /// [`DataMessage::developer_fields`]).
    pub fn resolve<'a, 'd>(
        &'d self,
        msg: &DataMessage<'a>,
    ) -> impl Iterator<Item = ResolvedField<'a, 'd>> + use<'a, 'd> {
        msg.developer_fields().filter_map(move |field| {
            let description = self.description(field.developer_data_index(), field.number())?;
            Some(ResolvedField { description, field })
        })
    }
}

fn uint<T: TryFrom<i64>>(msg: &DataMessage<'_>, number: u8) -> Option<T> {
    T::try_from(msg.field(number)?.value()?.as_i64()?).ok()
}

fn string(msg: &DataMessage<'_>, number: u8) -> Option<String> {
    match msg.field(number)?.value()? {
        Value::String(s) => s.to_str().ok().map(String::from),
        _ => None,
    }
}

fn parse_description(msg: &DataMessage<'_>) -> Option<FieldDescription> {
    let base_type = BaseType::from_byte(uint(msg, FD_FIT_BASE_TYPE_ID)?)?;
    Some(FieldDescription {
        developer_data_index: uint(msg, FD_DEVELOPER_DATA_INDEX)?,
        field_number: uint(msg, FD_FIELD_DEFINITION_NUMBER)?,
        base_type,
        name: string(msg, FD_FIELD_NAME),
        units: string(msg, FD_UNITS),
        scale: uint(msg, FD_SCALE),
        offset: uint(msg, FD_OFFSET),
        native_message: uint(msg, FD_NATIVE_MESG_NUM),
        native_field: uint(msg, FD_NATIVE_FIELD_NUM),
    })
}
