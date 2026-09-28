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

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;
