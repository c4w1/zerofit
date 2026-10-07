//! Typed FIT profile layer for [`zerofit`].
//!
//! `zerofit` decodes the FIT *protocol*: messages are numbers and fields are
//! raw values. This crate adds the FIT *profile* for the messages an activity
//! file is made of (`file_id`, `record`, `lap`, `session`, `event`,
//! `device_info`, `activity`, `hrv`): named accessors with scale and offset
//! applied, units in the docs, and Rust enums for enumerated types.
//!
//! Everything in [`messages`] and [`types`] is generated from the FIT SDK's
//! `Profile.xlsx` by `zerofit-codegen`; see the README for the license
//! notice. Like `zerofit`, the crate is `no_std`, never allocates and never
//! copies: the typed views are thin wrappers around
//! [`zerofit::DataMessage`].
//!
//! # Example
//!
//! ```
//! # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
//! use zerofit::{Decoder, Record as RawRecord};
//! use zerofit_profile::{Message, messages::Record};
//!
//! for item in Decoder::new(bytes) {
//!     let RawRecord::Data(msg) = item? else { continue };
//!     if let Some(record) = Record::new(msg) {
//!         println!("{:?} bpm at {:?}", record.heart_rate(), record.timestamp());
//!     }
//!     // Or dispatch on every covered message type:
//!     match Message::new(msg) {
//!         Message::Session(s) => println!("distance {:?} m", s.total_distance()),
//!         _ => {}
//!     }
//! }
//! # Ok::<(), zerofit::Error>(())
//! ```
//!
//! # Scaling
//!
//! The profile stores many values as scaled integers: `speed` is
//! `m/s * 1000`, `altitude` is `(m + 500) * 5`. Accessors for such fields
//! return `f64` in profile units, computed as `raw / scale - offset` (only
//! `/` and `-`, which `core` provides without `libm`).
//!
//! # Features
//!
//! - `alloc`: [`developer`], which resolves developer fields (fields added
//!   by apps such as Connect IQ data fields) using the file's
//!   `field_description` messages.
//!
//! # Not (yet) covered
//!
//! Subfields (alternative meanings of a field selected by another field,
//! such as `event.data`) and component expansion are not generated; the
//! underlying field is still available through its accessor or
//! [`raw`](messages::Record::raw).
#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
pub mod developer;

use zerofit::{ArrayIter, Value};

#[allow(
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::match_same_arms,
    clippy::wildcard_imports,
    clippy::doc_lazy_continuation,
    clippy::needless_lifetimes,
    clippy::enum_variant_names
)]
mod generated {
    pub mod messages;
    pub mod types;
}

pub use generated::{messages, types};
pub use messages::{MESSAGES, Message};
pub use types::MesgNum;

/// The profile name of global message number `global`, for every message in
/// the profile (not only those with typed views).
///
/// Manufacturer-specific messages (`0xFF00` and up) have no name, even though
/// the profile's `mesg_num` type labels the ends of that range
/// (`mfg_range_min`, `mfg_range_max`).
///
/// ```
/// assert_eq!(zerofit_profile::message_name(20), Some("record"));
/// assert_eq!(zerofit_profile::message_name(0xFF00), None);
/// assert_eq!(zerofit_profile::message_name(0xFFF0), None);
/// ```
#[must_use]
pub const fn message_name(global: u16) -> Option<&'static str> {
    match MesgNum::from_raw(global) {
        MesgNum::MfgRangeMin | MesgNum::MfgRangeMax => None,
        other => other.name(),
    }
}

/// Metadata for the fields of `global`, if this crate covers that message.
///
/// ```
/// let record = zerofit_profile::message_info(20).unwrap();
/// let speed = record.field(6).unwrap();
/// assert_eq!(speed.name(), "speed");
/// assert_eq!(speed.units(), Some("m/s"));
/// assert_eq!(speed.scale(), 1000.0);
/// ```
#[must_use]
pub fn message_info(global: u16) -> Option<&'static MessageInfo> {
    MESSAGES.iter().find(|m| m.number() == global)
}

/// Profile metadata for one message.
///
/// ```
/// let lap = zerofit_profile::message_info(19).unwrap();
/// assert_eq!(lap.name(), "lap");
/// assert!(lap.fields().iter().any(|f| f.name() == "total_distance"));
/// ```
#[derive(Debug)]
pub struct MessageInfo {
    number: u16,
    name: &'static str,
    fields: &'static [FieldInfo],
    /// Field number -> position in `fields`, `u8::MAX` if absent.
    index: &'static [u8; 256],
}

impl MessageInfo {
    pub(crate) const fn new(
        number: u16,
        name: &'static str,
        fields: &'static [FieldInfo],
        index: &'static [u8; 256],
    ) -> Self {
        Self {
            number,
            name,
            fields,
            index,
        }
    }

    /// Global message number.
    #[must_use]
    pub const fn number(&self) -> u16 {
        self.number
    }

    /// Profile name, e.g. `record`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// All fields, in profile order.
    #[must_use]
    pub const fn fields(&self) -> &'static [FieldInfo] {
        self.fields
    }

    /// The field with definition number `number`.
    ///
    /// Optimization: decoders call this for every field of every message
    /// (to scale it or name it), and `session` has over 150 fields, so it
    /// was a linear search on the hot path of profile-aware decoding. The
    /// generator emits a 256-entry index per message (2 KiB in total),
    /// making this one table load. Measured gain on the fixtures: up to 26%
    /// for scaling every field (`zerofit-bench`, `zerofit_profile_fields`);
    /// field *decoding* dominates the rest of that workload.
    #[must_use]
    pub fn field(&self, number: u8) -> Option<&'static FieldInfo> {
        let position = *self.index.get(usize::from(number))?;
        self.fields.get(usize::from(position))
    }
}

/// Profile metadata for one field.
///
/// ```
/// let distance = zerofit_profile::message_info(18).unwrap().field(9).unwrap();
/// assert_eq!(distance.name(), "total_distance");
/// assert_eq!((distance.units(), distance.scale()), (Some("m"), 100.0));
/// ```
#[derive(Debug)]
pub struct FieldInfo {
    number: u8,
    name: &'static str,
    profile_type: &'static str,
    units: Option<&'static str>,
    scale: f64,
    offset: f64,
    is_array: bool,
}

impl FieldInfo {
    pub(crate) const fn new(
        number: u8,
        name: &'static str,
        profile_type: &'static str,
        units: Option<&'static str>,
        scale: f64,
        offset: f64,
        is_array: bool,
    ) -> Self {
        Self {
            number,
            name,
            profile_type,
            units,
            scale,
            offset,
            is_array,
        }
    }

    /// Field definition number.
    #[must_use]
    pub const fn number(&self) -> u8 {
        self.number
    }

    /// Profile name, e.g. `heart_rate`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Profile type name: a base type (`uint8`) or a named type (`sport`).
    #[must_use]
    pub const fn profile_type(&self) -> &'static str {
        self.profile_type
    }

    /// Units, e.g. `m/s`.
    #[must_use]
    pub const fn units(&self) -> Option<&'static str> {
        self.units
    }

    /// Scale (1 if unscaled). The value is `raw / scale - offset`.
    #[must_use]
    pub const fn scale(&self) -> f64 {
        self.scale
    }

    /// Offset (0 if none). The value is `raw / scale - offset`.
    #[must_use]
    pub const fn offset(&self) -> f64 {
        self.offset
    }

    /// Whether the profile declares the field as an array.
    #[must_use]
    pub const fn is_array(&self) -> bool {
        self.is_array
    }

    /// Applies this field's scale and offset to a numeric value. Returns
    /// `None` for non-numeric values and invalid sentinels.
    ///
    /// ```
    /// use zerofit::Value;
    ///
    /// let altitude = zerofit_profile::message_info(20).unwrap().field(2).unwrap();
    /// assert_eq!(altitude.scaled(&Value::UInt16(3000)), Some(100.0)); // 3000 / 5 - 500
    /// ```
    #[must_use]
    pub fn scaled(&self, value: &Value<'_>) -> Option<f64> {
        if value.is_invalid() {
            return None;
        }
        Some(value.as_f64()? / self.scale - self.offset)
    }
}

/// The valid elements of an array field, scaled to `f64`.
///
/// Returned by accessors of array fields, such as
/// [`Hrv::time`](messages::Hrv::time). A field holding a single element
/// yields that element.
///
/// ```
/// use zerofit::encode::{Encoder, FileOptions};
/// use zerofit::{Decoder, Endian, FieldDefinition, Record};
/// use zerofit_profile::messages::Hrv;
///
/// // An hrv message with three beat intervals (s * 1000), one invalid.
/// let mut enc = Encoder::new();
/// enc.begin_file(FileOptions::new(2132))?;
/// enc.write_definition(0, Endian::Little, 78, &[FieldDefinition::new(0, 6, 0x84)], &[])?;
/// enc.write_data(0, &[0x20, 0x03, 0xFF, 0xFF, 0xE2, 0x04])?; // 800, invalid, 1250
/// let bytes = enc.finish()?;
///
/// for record in Decoder::new(&bytes) {
///     if let Ok(Record::Data(msg)) = record {
///         let intervals: Vec<f64> = Hrv::new(msg).unwrap().time().collect();
///         assert_eq!(intervals, [0.8, 1.25]);
///     }
/// }
/// # Ok::<(), zerofit::encode::EncodeError>(())
/// ```
#[derive(Debug, Clone)]
pub struct Elements<'a> {
    state: ElementsState<'a>,
    scale: f64,
    offset: f64,
}

#[derive(Debug, Clone)]
enum ElementsState<'a> {
    Many(ArrayIter<'a>),
    One(Option<Value<'a>>),
}

impl Iterator for Elements<'_> {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        loop {
            let value = match &mut self.state {
                ElementsState::Many(iter) => iter.next()?,
                ElementsState::One(value) => value.take()?,
            };
            if value.is_invalid() {
                continue;
            }
            if let Some(v) = value.as_f64() {
                return Some(v / self.scale - self.offset);
            }
        }
    }
}

/// Field readers shared by the generated accessors.
mod read {
    use zerofit::{DataMessage, FitStr, Value};

    use crate::{Elements, ElementsState};

    /// A field's valid value, using the first element if the device wrote
    /// an array where the profile has a scalar (as the FIT SDK does).
    fn scalar<'a>(msg: &DataMessage<'a>, number: u8) -> Option<Value<'a>> {
        match msg.field(number)?.value()? {
            Value::Array(a) => a.get(0).filter(|v| !v.is_invalid()),
            v => Some(v),
        }
    }

    /// A field as an integer of type `T`, whatever base type the device
    /// actually used, if it fits.
    pub(crate) fn int<T: TryFrom<i64>>(msg: &DataMessage<'_>, number: u8) -> Option<T> {
        T::try_from(scalar(msg, number)?.as_i64()?).ok()
    }

    pub(crate) fn float(msg: &DataMessage<'_>, number: u8) -> Option<f64> {
        scalar(msg, number)?.as_f64()
    }

    pub(crate) fn scaled(
        msg: &DataMessage<'_>,
        number: u8,
        scale: f64,
        offset: f64,
    ) -> Option<f64> {
        Some(float(msg, number)? / scale - offset)
    }

    pub(crate) fn string<'a>(msg: &DataMessage<'a>, number: u8) -> Option<FitStr<'a>> {
        match msg.field(number)?.value()? {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn bytes<'a>(msg: &DataMessage<'a>, number: u8) -> Option<&'a [u8]> {
        let field = msg.field(number)?;
        field.value().map(|_| field.bytes())
    }

    // Emitted by the generator for non-numeric array fields; none of the
    // current messages has one.
    #[allow(dead_code)]
    pub(crate) fn raw<'a>(msg: &DataMessage<'a>, number: u8) -> Option<Value<'a>> {
        msg.field(number)?.value()
    }

    pub(crate) fn elements<'a>(
        msg: &DataMessage<'a>,
        number: u8,
        scale: f64,
        offset: f64,
    ) -> Elements<'a> {
        let state = match msg.field(number).map(|f| f.raw_value()) {
            Some(Value::Array(a)) => ElementsState::Many(a.iter()),
            other => ElementsState::One(other),
        };
        Elements {
            state,
            scale,
            offset,
        }
    }
}
