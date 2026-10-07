//! The profile subset, as stored in `profile-subset.json`.

use serde::{Deserialize, Serialize};

/// The part of the FIT profile that `zerofit-profile` is generated from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// FIT SDK release the subset was extracted from, e.g. `21.171`.
    pub sdk_version: String,
    /// Every global message number in the profile, name to number.
    pub mesg_num: Vec<EnumValue>,
    /// Types referenced by the selected messages' fields.
    pub types: Vec<TypeDef>,
    /// The selected messages, in profile order.
    pub messages: Vec<Message>,
}

/// A named type from the profile's `Types` sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDef {
    /// Type name, e.g. `sport`.
    pub name: String,
    /// Base type name, e.g. `enum` or `uint16`.
    pub base_type: String,
    /// Named values, in profile order.
    pub values: Vec<EnumValue>,
}

/// One named value of a type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumValue {
    /// Value name, e.g. `cycling`.
    pub name: String,
    /// Numeric value.
    pub value: u64,
    /// Profile comment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// A message from the profile's `Messages` sheet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Message name, e.g. `record`.
    pub name: String,
    /// Global message number.
    pub number: u16,
    /// Fields, in profile order.
    pub fields: Vec<Field>,
}

/// A field of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// Field definition number.
    pub number: u8,
    /// Field name, e.g. `heart_rate`.
    pub name: String,
    /// Profile type: a base type (`uint8`) or a named type (`sport`).
    pub field_type: String,
    /// Array marker as written in the profile (`[N]`, `[3]`), if an array.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub array: Option<String>,
    /// Component field names this field expands into.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,
    /// Scale, one per component (or one for the field).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scale: Vec<f64>,
    /// Offset, one per component (or one for the field).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset: Vec<f64>,
    /// Units, e.g. `m/s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// Profile comment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Alternative interpretations selected by another field's value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subfields: Vec<SubField>,
}

/// A subfield: the same bytes as its parent field, interpreted differently
/// when a reference field has a given value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubField {
    /// Subfield name, e.g. `timer_trigger`.
    pub name: String,
    /// Profile type.
    pub field_type: String,
    /// Scale, one per component (or one for the subfield).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scale: Vec<f64>,
    /// Offset, one per component (or one for the subfield).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offset: Vec<f64>,
    /// Units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    /// (reference field name, reference value name) pairs; the subfield
    /// applies when any pair matches.
    pub references: Vec<(String, String)>,
}

impl Field {
    /// The field's own scale and offset, if it has exactly one of each
    /// (fields with several components are scaled per component instead).
    #[must_use]
    pub fn single_scale_offset(&self) -> Option<(f64, f64)> {
        if self.components.len() > 1 {
            return None;
        }
        let scale = match self.scale.as_slice() {
            [] => 1.0,
            [s] => *s,
            _ => return None,
        };
        let offset = match self.offset.as_slice() {
            [] => 0.0,
            [o] => *o,
            _ => return None,
        };
        #[allow(clippy::float_cmp)] // exact profile constants
        let trivial = scale == 1.0 && offset == 0.0;
        (!trivial).then_some((scale, offset))
    }
}
