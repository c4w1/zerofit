//! The FIT CRC-16.
//!
//! FIT files protect the 14-byte file header and the whole file with a 16-bit
//! CRC. The algorithm is CRC-16/ARC: reflected polynomial `0xA001`
//! (`0x8005` unreflected), initial value `0`, no final XOR. The FIT SDK
//! specifies it with a 16-entry nibble table; this module computes the same
//! function eight bytes at a time ("slicing-by-8", see [`Crc16::update`])
//! with tables built at compile time.
//!
//! A useful property: appending a message's CRC to the message, little-endian,
//! makes the CRC of the combined bytes zero.
//!
//! ```
//! use zerofit::crc::crc16;
//!
//! let data = b"123456789";
//! let crc = crc16(data);
//! assert_eq!(crc, 0xBB3D);
//!
//! let mut with_crc = data.to_vec();
//! with_crc.extend_from_slice(&crc.to_le_bytes());
//! assert_eq!(crc16(&with_crc), 0);
//! ```

/// Reflected CRC-16/ARC polynomial.
const POLY: u16 = 0xA001;

/// Slicing-by-8 lookup tables, built at compile time (4 KiB).
///
/// `TABLES[0]` is the classic byte-wise table: the CRC contribution of one
/// byte. `TABLES[k][b]` is the contribution of byte `b` followed by `k` zero
/// bytes, i.e. `TABLES[0]` applied `k` more times. Because a CRC is linear
/// (XOR distributes over it), the CRC update for 8 bytes is the XOR of the
/// eight bytes' contributions, each looked up at its distance from the end.
static TABLES: [[u16; 256]; 8] = build_tables();

// Evaluated at compile time only: an overflow or out-of-bounds index here
// would be a build error, never a runtime panic.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing
)]
const fn build_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u16;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

// Compile time only, as above.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
const fn build_tables() -> [[u16; 256]; 8] {
    let mut tables = [[0u16; 256]; 8];
    tables[0] = build_table();
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = tables[k - 1][i];
            tables[k][i] = (prev >> 8) ^ tables[0][(prev & 0xFF) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// `TABLES[k][byte]`. Both indices are in range by construction (`k < 8`,
/// `byte <= 255`), so the bounds checks and the fallback compile away.
#[inline]
fn t(k: usize, byte: u8) -> u16 {
    TABLES
        .get(k)
        .and_then(|table| table.get(usize::from(byte)))
        .copied()
        .unwrap_or(0)
}

/// One byte, the classic table-driven step.
#[inline]
fn step(crc: u16, byte: u8) -> u16 {
    (crc >> 8) ^ t(0, crc.to_le_bytes()[0] ^ byte)
}

/// Eight bytes at once.
#[inline]
fn step8(crc: u16, block: [u8; 8]) -> u16 {
    let [c0, c1] = crc.to_le_bytes();
    let [b0, b1, b2, b3, b4, b5, b6, b7] = block;
    // The 16-bit CRC state is XORed into the first two bytes; then each byte
    // contributes its table entry for its distance from the end of the block.
    t(7, b0 ^ c0) ^ t(6, b1 ^ c1) ^ t(5, b2) ^ t(4, b3) ^ t(3, b4) ^ t(2, b5) ^ t(1, b6) ^ t(0, b7)
}

/// Computes the FIT CRC-16 of `data` in one call.
///
/// ```
/// assert_eq!(zerofit::crc::crc16(b""), 0);
/// assert_eq!(zerofit::crc::crc16(b"123456789"), 0xBB3D);
/// ```
#[must_use]
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = Crc16::new();
    crc.update(data);
    crc.value()
}

/// Incremental FIT CRC-16 state, for data that arrives in pieces.
///
/// Feeding the same bytes in any split produces the same result as
/// [`crc16`].
///
/// ```
/// use zerofit::crc::{crc16, Crc16};
///
/// let mut crc = Crc16::new();
/// crc.update(b"1234");
/// crc.update(b"56789");
/// assert_eq!(crc.value(), crc16(b"123456789"));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Crc16 {
    state: u16,
}

impl Crc16 {
    /// Creates a CRC state with the FIT initial value (`0`).
    #[must_use]
    pub const fn new() -> Self {
        Self { state: 0 }
    }

    /// Feeds `data` into the CRC.
    ///
    /// Optimization: the byte-at-a-time loop is a chain of dependent
    /// operations (each step needs the previous CRC before it can load the
    /// next table entry), so it runs at the latency of a load + shift + XOR
    /// per byte no matter how wide the CPU is. Slicing-by-8 does eight
    /// *independent* table loads per 8-byte block and combines them with
    /// XOR, so the loads overlap and the dependency chain is one step per 8
    /// bytes. The file CRC is checked over every byte before decoding, so it
    /// dominated the cost of iterating records (about 80% of it). Measured
    /// on the fixtures, iterating every record with CRC validation became
    /// 2.2-3.4x faster (`zerofit-bench`, `zerofit_records`). The result is
    /// identical (tested against the FIT SDK's nibble algorithm in
    /// `matches_reference`).
    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        let mut crc = self.state;
        let mut blocks = data.chunks_exact(8);
        for block in &mut blocks {
            if let Ok(block) = <[u8; 8]>::try_from(block) {
                crc = step8(crc, block);
            }
        }
        for &b in blocks.remainder() {
            crc = step(crc, b);
        }
        self.state = crc;
    }

    /// Returns the CRC of all bytes fed so far.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::vec::Vec;

    /// The nibble-table algorithm exactly as given in the FIT SDK
    /// documentation, used as an independent reference.
    fn reference(data: &[u8]) -> u16 {
        const T: [u16; 16] = [
            0x0000, 0xCC01, 0xD801, 0x1400, 0xF001, 0x3C00, 0x2800, 0xE401, 0xA001, 0x6C00, 0x7800,
            0xB401, 0x5000, 0x9C01, 0x8801, 0x4400,
        ];
        let mut crc: u16 = 0;
        for &byte in data {
            let tmp = T[usize::from(crc & 0xF)];
            crc = (crc >> 4) & 0x0FFF;
            crc = crc ^ tmp ^ T[usize::from(byte & 0xF)];
            let tmp = T[usize::from(crc & 0xF)];
            crc = (crc >> 4) & 0x0FFF;
            crc = crc ^ tmp ^ T[usize::from(byte >> 4)];
        }
        crc
    }

    #[test]
    fn known_vectors() {
        assert_eq!(crc16(b""), 0x0000);
        assert_eq!(crc16(b"123456789"), 0xBB3D);
        assert_eq!(crc16(&[0x00]), 0x0000);
        assert_eq!(crc16(&[0xFF]), 0x4040);
    }

    #[test]
    fn table_matches_sdk_nibble_table() {
        // Entry 16*k of the byte table equals entry k of the SDK nibble table
        // for the low nibble; checking all 256 single-byte inputs covers it.
        for b in 0..=255u8 {
            assert_eq!(crc16(&[b]), reference(&[b]), "byte {b:#04x}");
        }
    }

    proptest! {
        #[test]
        fn matches_reference(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            prop_assert_eq!(crc16(&data), reference(&data));
        }

        #[test]
        fn incremental_equals_one_shot(
            data in proptest::collection::vec(any::<u8>(), 0..1024),
            split in any::<prop::sample::Index>(),
        ) {
            let at = split.index(data.len() + 1);
            let (a, b) = data.split_at(at);
            let mut crc = Crc16::new();
            crc.update(a);
            crc.update(b);
            prop_assert_eq!(crc.value(), crc16(&data));
        }

        #[test]
        fn appended_crc_gives_zero_residue(data in proptest::collection::vec(any::<u8>(), 0..1024)) {
            let mut buf: Vec<u8> = data.clone();
            buf.extend_from_slice(&crc16(&data).to_le_bytes());
            prop_assert_eq!(crc16(&buf), 0);
        }

        #[test]
        fn single_bit_flip_is_detected(
            data in proptest::collection::vec(any::<u8>(), 1..512),
            pos in any::<prop::sample::Index>(),
            bit in 0u8..8,
        ) {
            let mut flipped = data.clone();
            let i = pos.index(flipped.len());
            flipped[i] ^= 1 << bit;
            prop_assert_ne!(crc16(&flipped), crc16(&data));
        }
    }
}
