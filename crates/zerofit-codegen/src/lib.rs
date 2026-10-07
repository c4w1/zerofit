//! Generates the `zerofit-profile` crate's typed layer from the FIT profile.
//!
//! Two steps, so CI can verify the generated code without the FIT SDK:
//!
//! 1. [`extract`] reads Garmin's `Profile.xlsx` (not redistributable) and
//!    keeps only what `zerofit-profile` needs: the selected messages, the
//!    types they use, and the `mesg_num` table. The result is a
//!    [`Profile`] serialized as `crates/zerofit-profile/codegen/profile-subset.json`.
//! 2. [`generate`] turns that subset into Rust source.
//!
//! Driven by `cargo xtask extract` and `cargo xtask codegen [--check]`.

// An internal build tool: panics abort the tool, not a user's program.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc
)]

mod extract;
mod generate;
mod model;

pub use extract::{MESSAGES, extract};
pub use generate::{GeneratedFile, generate};
pub use model::{EnumValue, Field, Message, Profile, SubField, TypeDef};
