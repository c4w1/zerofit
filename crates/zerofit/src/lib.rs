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
mod error;
mod header;
mod value;

pub use base_type::{BaseType, Endian};
pub use error::{Error, ErrorKind};
pub use header::{FileHeader, ProtocolVersion};
pub use value::{Array, ArrayIter, FitStr, Value};
