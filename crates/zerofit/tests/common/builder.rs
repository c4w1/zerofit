//! A minimal FIT *writer* for building synthetic test files.
//!
//! Shared between unit tests (included with `#[path]`) and integration tests,
//! so it depends on nothing but `std`. It carries its own bitwise CRC
//! implementation so tests do not validate the library's CRC against itself.
//!
//! The builder writes exactly what it is told, so tests can produce malformed
//! files: wrong CRCs, wrong data sizes, undefined local messages, and so on.

#![allow(
    dead_code,
    unreachable_pub,
    missing_docs,
    clippy::cast_possible_truncation
)]

use std::vec::Vec;

/// Bitwise CRC-16/ARC (reflected poly 0xA001, init 0), the FIT CRC.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// How to fill in the 14-byte header's CRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderCrc {
    /// Correct CRC.
    Valid,
    /// 0x0000, meaning "not computed".
    Zero,
    /// A specific (probably wrong) value.
    Value(u16),
}

/// One field definition: (field number, size in bytes, base type byte).
pub type Field = (u8, u8, u8);

/// Builds FIT files record by record.
#[derive(Debug, Clone)]
pub struct FitBuilder {
    header_size: u8,
    protocol_version: u8,
    profile_version: u16,
    header_crc: HeaderCrc,
    data_size: Option<u32>,
    file_crc: Option<u16>,
    records: Vec<u8>,
}

impl Default for FitBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FitBuilder {
    /// A 14-byte-header file, protocol 2.0, profile 21.32.
    pub fn new() -> Self {
        Self {
            header_size: 14,
            protocol_version: 0x20,
            profile_version: 2132,
            header_crc: HeaderCrc::Valid,
            data_size: None,
            file_crc: None,
            records: Vec::new(),
        }
    }

    /// Use a 12-byte legacy header (no header CRC).
    pub fn legacy_header(mut self) -> Self {
        self.header_size = 12;
        self
    }

    pub fn protocol_version(mut self, v: u8) -> Self {
        self.protocol_version = v;
        self
    }

    pub fn header_crc(mut self, crc: HeaderCrc) -> Self {
        self.header_crc = crc;
        self
    }

    /// Override the data size written in the header.
    pub fn data_size(mut self, size: u32) -> Self {
        self.data_size = Some(size);
        self
    }

    /// Override the file CRC written at the end.
    pub fn file_crc(mut self, crc: u16) -> Self {
        self.file_crc = Some(crc);
        self
    }

    /// Append a definition message without developer fields.
    pub fn definition(
        &mut self,
        local: u8,
        big_endian: bool,
        global: u16,
        fields: &[Field],
    ) -> &mut Self {
        self.definition_with_dev(local, big_endian, global, fields, None)
    }

    /// Append a definition message; `dev` is `Some` to set the developer data
    /// flag, with (field number, size, developer data index) triples.
    pub fn definition_with_dev(
        &mut self,
        local: u8,
        big_endian: bool,
        global: u16,
        fields: &[Field],
        dev: Option<&[Field]>,
    ) -> &mut Self {
        let flag = if dev.is_some() { 0x20 } else { 0 };
        self.records.push(0x40 | flag | (local & 0x0F));
        self.records.push(0);
        self.records.push(u8::from(big_endian));
        let g = if big_endian {
            global.to_be_bytes()
        } else {
            global.to_le_bytes()
        };
        self.records.extend_from_slice(&g);
        self.records.push(fields.len() as u8);
        for &(n, s, t) in fields {
            self.records.extend_from_slice(&[n, s, t]);
        }
        if let Some(dev) = dev {
            self.records.push(dev.len() as u8);
            for &(n, s, i) in dev {
                self.records.extend_from_slice(&[n, s, i]);
            }
        }
        self
    }

    /// Append a normal data message with the given payload bytes.
    pub fn data(&mut self, local: u8, payload: &[u8]) -> &mut Self {
        self.records.push(local & 0x0F);
        self.records.extend_from_slice(payload);
        self
    }

    /// Append a compressed-timestamp data message.
    pub fn compressed(&mut self, local: u8, time_offset: u8, payload: &[u8]) -> &mut Self {
        self.records
            .push(0x80 | ((local & 0x03) << 5) | (time_offset & 0x1F));
        self.records.extend_from_slice(payload);
        self
    }

    /// Append arbitrary bytes to the record section.
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.records.extend_from_slice(bytes);
        self
    }

    /// Current length of the record section.
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// Serialize: header, records, file CRC.
    pub fn build(&self) -> Vec<u8> {
        let data_size = self.data_size.unwrap_or(self.records.len() as u32);
        let mut out = Vec::with_capacity(self.records.len() + 16);
        out.push(self.header_size);
        out.push(self.protocol_version);
        out.extend_from_slice(&self.profile_version.to_le_bytes());
        out.extend_from_slice(&data_size.to_le_bytes());
        out.extend_from_slice(b".FIT");
        if self.header_size == 14 {
            let crc = match self.header_crc {
                HeaderCrc::Valid => crc16(&out),
                HeaderCrc::Zero => 0,
                HeaderCrc::Value(v) => v,
            };
            out.extend_from_slice(&crc.to_le_bytes());
        }
        out.extend_from_slice(&self.records);
        let crc = self.file_crc.unwrap_or_else(|| crc16(&out));
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }
}
