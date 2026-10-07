//! A minimal FIT encoder.
//!
//! [`Encoder`] writes FIT files record by record and fills in everything that
//! depends on the whole file when the file ends: the header's data size, the
//! header CRC and the file CRC. It checks that every data message matches the
//! size declared by its definition, so its output always decodes.
//!
//! The main use is rewriting: [`Encoder::write_record`] accepts the
//! [`Record`]s produced by a [`Decoder`](crate::Decoder), so a file can be
//! decoded, edited and re-encoded with valid CRCs. Re-encoding an unmodified
//! file reproduces it byte for byte (as long as its CRCs were correct).
//!
//! ```
//! use zerofit::encode::{Encoder, FileOptions};
//! use zerofit::{BaseType, Decoder, Endian, FieldDefinition, Record};
//!
//! let mut enc = Encoder::new();
//! enc.begin_file(FileOptions::new(2132))?;
//! // Local message 0 = global message 20 (`record`): timestamp + heart rate.
//! enc.write_definition(0, Endian::Little, 20, &[
//!     FieldDefinition::new(253, 4, BaseType::UInt32.to_byte()),
//!     FieldDefinition::new(3, 1, BaseType::UInt8.to_byte()),
//! ], &[])?;
//! let mut payload = 1_000_000_000u32.to_le_bytes().to_vec();
//! payload.push(142);
//! enc.write_data(0, &payload)?;
//! let bytes = enc.finish()?;
//!
//! // The output decodes, CRCs included.
//! let messages = Decoder::new(&bytes)
//!     .filter(|r| matches!(r, Ok(Record::Data(_))))
//!     .count();
//! assert_eq!(messages, 1);
//! # Ok::<(), zerofit::encode::EncodeError>(())
//! ```

use alloc::vec::Vec;

use crate::base_type::Endian;
use crate::crc::crc16;
use crate::decoder::Record;
use crate::definition::{DeveloperFieldDefinition, FieldDefinition};
use crate::header::{FileHeader, ProtocolVersion};

/// Number of local message types a normal record header can address.
const LOCAL_TYPES: usize = 16;
/// Highest local message type a compressed timestamp header can address.
const MAX_COMPRESSED_LOCAL: u8 = 3;
/// Highest time offset a compressed timestamp header can carry.
const MAX_TIME_OFFSET: u8 = 0x1F;
/// Size of a header with CRC.
const HEADER_SIZE_WITH_CRC: u8 = 14;
/// Size of a legacy header.
const LEGACY_HEADER_SIZE: u8 = 12;
/// Offset of the data size within the header.
const DATA_SIZE_OFFSET: usize = 4;
/// Bytes of the header covered by the header CRC.
const HEADER_CRC_COVERS: usize = 12;

/// An error from the [`Encoder`]. Encoding errors are caller mistakes, so
/// they carry the offending values rather than a byte offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum EncodeError {
    /// A record was written with no file open (call
    /// [`Encoder::begin_file`] first).
    #[error("no FIT file is open")]
    NoOpenFile,
    /// [`Encoder::begin_file`] was called while a file was open.
    #[error("a FIT file is already open")]
    FileAlreadyOpen,
    /// The local message type is out of range for the record header used
    /// (0-15 normally, 0-3 with a compressed timestamp).
    #[error("local message type {0} is out of range")]
    InvalidLocalMessageType(u8),
    /// A compressed timestamp offset is above 31.
    #[error("time offset {0} does not fit in 5 bits")]
    InvalidTimeOffset(u8),
    /// A data message uses a local message type with no definition.
    #[error("local message type {0} used before being defined")]
    UndefinedLocalMessage(u8),
    /// A data message's length differs from its definition's message size.
    #[error("local message type {local}: expected {expected} bytes, got {actual}")]
    MessageSizeMismatch {
        /// Local message type.
        local: u8,
        /// Size required by the definition.
        expected: usize,
        /// Size supplied.
        actual: usize,
    },
    /// A definition has more than 255 regular or developer fields.
    #[error("{0} fields do not fit in a definition message (max 255)")]
    TooManyFields(usize),
    /// The file's records exceed the 4 GiB a header can describe.
    #[error("data section exceeds u32::MAX bytes")]
    DataTooLarge,
}

/// Header settings for one encoded file.
///
/// ```
/// use zerofit::encode::FileOptions;
/// use zerofit::ProtocolVersion;
///
/// let options = FileOptions::new(2132)
///     .protocol_version(ProtocolVersion::from_byte(0x10))
///     .legacy_header();
/// # let _ = options;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileOptions {
    header_size: u8,
    protocol_version: ProtocolVersion,
    profile_version: u16,
    header_crc: bool,
}

impl FileOptions {
    /// A 14-byte header with a computed CRC, protocol 2.0 and the given
    /// profile version (for example `2132` for profile 21.32).
    #[must_use]
    pub const fn new(profile_version: u16) -> Self {
        Self {
            header_size: HEADER_SIZE_WITH_CRC,
            protocol_version: ProtocolVersion::from_byte(0x20),
            profile_version,
            header_crc: true,
        }
    }

    /// Settings that reproduce an existing header: same size, versions, and
    /// whether the header CRC was computed (a stored `0x0000` is kept).
    #[must_use]
    pub const fn from_header(header: &FileHeader) -> Self {
        Self {
            header_size: header.size(),
            protocol_version: header.protocol_version(),
            profile_version: header.profile_version(),
            header_crc: !matches!(header.crc(), Some(0)),
        }
    }

    /// Use a 12-byte legacy header, which has no CRC.
    #[must_use]
    pub const fn legacy_header(mut self) -> Self {
        self.header_size = LEGACY_HEADER_SIZE;
        self
    }

    /// Sets the protocol version byte.
    #[must_use]
    pub const fn protocol_version(mut self, version: ProtocolVersion) -> Self {
        self.protocol_version = version;
        self
    }

    /// Whether to compute the 14-byte header's CRC (default `true`). If
    /// `false`, `0x0000` ("not computed") is written.
    #[must_use]
    pub const fn header_crc(mut self, compute: bool) -> Self {
        self.header_crc = compute;
        self
    }
}

/// Writes FIT files. See the [module documentation](self).
///
/// Several files can be written back to back (a chained FIT file) by calling
/// [`begin_file`](Self::begin_file) and [`end_file`](Self::end_file) for each.
#[derive(Debug, Clone, Default)]
pub struct Encoder {
    out: Vec<u8>,
    open: Option<OpenFile>,
    /// Message size declared by each local message type's definition.
    sizes: [Option<usize>; LOCAL_TYPES],
}

#[derive(Debug, Clone, Copy)]
struct OpenFile {
    start: usize,
    options: FileOptions,
}

impl Encoder {
    /// Creates an encoder with an empty output buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new file, writing a placeholder header that
    /// [`end_file`](Self::end_file) completes. Clears all definitions.
    ///
    /// # Errors
    ///
    /// [`EncodeError::FileAlreadyOpen`] if the previous file was not ended.
    pub fn begin_file(&mut self, options: FileOptions) -> Result<(), EncodeError> {
        if self.open.is_some() {
            return Err(EncodeError::FileAlreadyOpen);
        }
        self.open = Some(OpenFile {
            start: self.out.len(),
            options,
        });
        self.sizes = [None; LOCAL_TYPES];
        self.out.push(options.header_size);
        self.out.push(options.protocol_version.to_byte());
        self.out
            .extend_from_slice(&options.profile_version.to_le_bytes());
        self.out.extend_from_slice(&[0; 4]); // data size, patched later
        self.out.extend_from_slice(b".FIT");
        if options.header_size == HEADER_SIZE_WITH_CRC {
            self.out.extend_from_slice(&[0; 2]); // header CRC, patched later
        }
        Ok(())
    }

    /// Writes a definition message binding `local` (0-15) to global message
    /// `global`. The developer data flag is set if `developer_fields` is not
    /// empty.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NoOpenFile`], [`EncodeError::InvalidLocalMessageType`],
    /// or [`EncodeError::TooManyFields`].
    pub fn write_definition(
        &mut self,
        local: u8,
        endian: Endian,
        global: u16,
        fields: &[FieldDefinition],
        developer_fields: &[DeveloperFieldDefinition],
    ) -> Result<(), EncodeError> {
        let mut raw = Vec::with_capacity(fields.len().saturating_mul(3));
        for f in fields {
            raw.extend_from_slice(&[f.number(), f.size(), f.base_type_byte()]);
        }
        let mut dev = Vec::with_capacity(developer_fields.len().saturating_mul(3));
        for f in developer_fields {
            dev.extend_from_slice(&[f.number(), f.size(), f.developer_data_index()]);
        }
        let dev = (!developer_fields.is_empty()).then_some(dev.as_slice());
        self.definition_raw(local, endian, global, &raw, dev)
    }

    /// Writes a definition from raw 3-byte field triples. `developer` is
    /// `Some` to set the developer data flag.
    fn definition_raw(
        &mut self,
        local: u8,
        endian: Endian,
        global: u16,
        fields: &[u8],
        developer: Option<&[u8]>,
    ) -> Result<(), EncodeError> {
        self.require_open()?;
        let slot = self
            .sizes
            .get_mut(usize::from(local))
            .ok_or(EncodeError::InvalidLocalMessageType(local))?;
        let count = |raw: &[u8]| {
            let n = raw.len() / 3;
            u8::try_from(n).map_err(|_| EncodeError::TooManyFields(n))
        };
        let field_count = count(fields)?;
        let dev_count = developer.map(count).transpose()?;

        let size_of = |raw: &[u8]| {
            raw.chunks_exact(3)
                .filter_map(|t| t.get(1))
                .map(|&s| usize::from(s))
                .fold(0, usize::saturating_add)
        };
        *slot = Some(size_of(fields).saturating_add(developer.map_or(0, size_of)));

        let flag = if developer.is_some() { 0x20 } else { 0 };
        self.out.push(0x40 | flag | local);
        let (arch, global) = match endian {
            Endian::Little => (0, global.to_le_bytes()),
            Endian::Big => (1, global.to_be_bytes()),
        };
        self.out.extend_from_slice(&[0, arch]);
        self.out.extend_from_slice(&global);
        self.out.push(field_count);
        self.out.extend_from_slice(fields);
        if let (Some(dev), Some(n)) = (developer, dev_count) {
            self.out.push(n);
            self.out.extend_from_slice(dev);
        }
        Ok(())
    }

    /// Writes a data message with a normal record header. `bytes` holds all
    /// field values in definition order and must match the definition's size.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NoOpenFile`], [`EncodeError::InvalidLocalMessageType`],
    /// [`EncodeError::UndefinedLocalMessage`], or
    /// [`EncodeError::MessageSizeMismatch`].
    pub fn write_data(&mut self, local: u8, bytes: &[u8]) -> Result<(), EncodeError> {
        self.check_data(local, bytes)?;
        self.out.push(local);
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    /// Writes a data message with a compressed timestamp header: `local`
    /// must be 0-3 and `time_offset` (the low 5 bits of the timestamp) 0-31.
    ///
    /// # Errors
    ///
    /// As [`write_data`](Self::write_data), plus
    /// [`EncodeError::InvalidTimeOffset`].
    pub fn write_compressed_data(
        &mut self,
        local: u8,
        time_offset: u8,
        bytes: &[u8],
    ) -> Result<(), EncodeError> {
        if local > MAX_COMPRESSED_LOCAL {
            return Err(EncodeError::InvalidLocalMessageType(local));
        }
        if time_offset > MAX_TIME_OFFSET {
            return Err(EncodeError::InvalidTimeOffset(time_offset));
        }
        self.check_data(local, bytes)?;
        self.out.push(0x80 | (local << 5) | time_offset);
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    fn check_data(&self, local: u8, bytes: &[u8]) -> Result<(), EncodeError> {
        self.require_open()?;
        let expected = self
            .sizes
            .get(usize::from(local))
            .ok_or(EncodeError::InvalidLocalMessageType(local))?
            .ok_or(EncodeError::UndefinedLocalMessage(local))?;
        if expected == bytes.len() {
            Ok(())
        } else {
            Err(EncodeError::MessageSizeMismatch {
                local,
                expected,
                actual: bytes.len(),
            })
        }
    }

    /// Re-encodes a record produced by a [`Decoder`](crate::Decoder):
    /// headers begin a file, definitions and data messages are copied
    /// exactly, and file ends end the file (with a freshly computed CRC).
    ///
    /// To edit a data message, write it with
    /// [`write_data`](Self::write_data) instead, using
    /// [`DataMessage::local_message_type`](crate::DataMessage::local_message_type)
    /// and modified [`bytes`](crate::DataMessage::bytes).
    ///
    /// # Errors
    ///
    /// Any error of the corresponding `write_*`, `begin_file` or `end_file`
    /// call.
    pub fn write_record(&mut self, record: &Record<'_>) -> Result<(), EncodeError> {
        match record {
            Record::Header(header) => self.begin_file(FileOptions::from_header(header)),
            Record::Definition(def) => self.definition_raw(
                def.local_message_type(),
                def.endian(),
                def.global_message_number(),
                def.raw_fields(),
                def.has_developer_data_flag()
                    .then(|| def.raw_developer_fields()),
            ),
            Record::Data(msg) => match msg.compressed_time_offset() {
                Some(offset) => {
                    self.write_compressed_data(msg.local_message_type(), offset, msg.bytes())
                }
                None => self.write_data(msg.local_message_type(), msg.bytes()),
            },
            Record::FileEnd { .. } => self.end_file(),
        }
    }

    /// Ends the open file: fills in the data size and header CRC, and appends
    /// the file CRC.
    ///
    /// # Errors
    ///
    /// [`EncodeError::NoOpenFile`], or [`EncodeError::DataTooLarge`] if the
    /// records exceed 4 GiB.
    pub fn end_file(&mut self) -> Result<(), EncodeError> {
        let file = self.open.ok_or(EncodeError::NoOpenFile)?;
        let options = file.options;
        let data_start = file.start.saturating_add(usize::from(options.header_size));
        let data_size = u32::try_from(self.out.len().saturating_sub(data_start))
            .map_err(|_| EncodeError::DataTooLarge)?;
        patch(
            &mut self.out,
            file.start.saturating_add(DATA_SIZE_OFFSET),
            &data_size.to_le_bytes(),
        );
        if options.header_size == HEADER_SIZE_WITH_CRC && options.header_crc {
            let covered = file.start.saturating_add(HEADER_CRC_COVERS);
            let crc = crc16(self.out.get(file.start..covered).unwrap_or_default());
            patch(&mut self.out, covered, &crc.to_le_bytes());
        }
        let crc = crc16(self.out.get(file.start..).unwrap_or_default());
        self.out.extend_from_slice(&crc.to_le_bytes());
        self.open = None;
        Ok(())
    }

    /// Ends the open file, if any, and returns the encoded bytes.
    ///
    /// # Errors
    ///
    /// As [`end_file`](Self::end_file).
    pub fn finish(mut self) -> Result<Vec<u8>, EncodeError> {
        if self.open.is_some() {
            self.end_file()?;
        }
        Ok(self.out)
    }

    fn require_open(&self) -> Result<(), EncodeError> {
        self.open.map(|_| ()).ok_or(EncodeError::NoOpenFile)
    }
}

/// Overwrites `out[at..at + bytes.len()]`. The range always exists because
/// the encoder wrote the placeholder itself; if it did not, nothing happens.
fn patch(out: &mut [u8], at: usize, bytes: &[u8]) {
    if let Some(dst) = out.get_mut(at..) {
        for (d, s) in dst.iter_mut().zip(bytes) {
            *d = *s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_type::BaseType;
    use crate::decoder::{DecodeOptions, Decoder};
    use crate::test_builder::{FitBuilder, HeaderCrc};
    use crate::value::Value;
    use proptest::prelude::*;
    use std::vec;

    fn reencode(bytes: &[u8]) -> Vec<u8> {
        let mut enc = Encoder::new();
        for record in Decoder::new(bytes) {
            enc.write_record(&record.unwrap()).unwrap();
        }
        enc.finish().unwrap()
    }

    /// Files covering every record kind; same shapes as the fuzz seeds.
    fn sample_files() -> Vec<Vec<u8>> {
        let mut out = Vec::new();

        let mut b = FitBuilder::new();
        b.definition(0, false, 0, &[(0, 1, 0x00), (4, 4, 0x86)])
            .data(0, &[4, 1, 2, 3, 4])
            .definition(1, false, 20, &[(253, 4, 0x86), (3, 1, 0x02)])
            .data(1, &[0, 0, 0, 0x40, 120])
            .compressed(1, 3, &[0xFF, 0xFF, 0xFF, 0xFF, 121]);
        out.push(b.build());

        let mut b = FitBuilder::new().legacy_header().protocol_version(0x10);
        b.definition(3, true, 20, &[(253, 4, 0x86), (6, 2, 0x84)])
            .data(3, &[0x40, 0, 0, 0, 0x01, 0x02]);
        out.push(b.build());

        let mut b = FitBuilder::new().header_crc(HeaderCrc::Zero);
        b.definition_with_dev(0, false, 20, &[(3, 1, 2)], Some(&[(0, 2, 0)]))
            .data(0, &[1, 2, 3])
            .definition_with_dev(1, false, 21, &[(0, 1, 0)], Some(&[]))
            .data(1, &[9]);
        out.push(b.build());

        let mut chained = out[0].clone();
        chained.extend_from_slice(&out[1]);
        out.push(chained);
        out
    }

    #[test]
    fn rewrite_is_byte_identical() {
        for file in sample_files() {
            assert_eq!(reencode(&file), file);
        }
    }

    #[test]
    fn rewrite_fixes_bad_crcs() {
        let good = &sample_files()[0];
        let mut bad = good.clone();
        let n = bad.len();
        bad[n - 1] ^= 0xFF;
        bad[12] ^= 0xFF;
        let lenient = DecodeOptions::new()
            .validate_file_crc(false)
            .validate_header_crc(false);
        let mut enc = Encoder::new();
        for r in Decoder::with_options(&bad, lenient) {
            enc.write_record(&r.unwrap()).unwrap();
        }
        assert_eq!(&enc.finish().unwrap(), good);
    }

    #[test]
    fn edited_message_decodes_with_valid_crcs() {
        let file = &sample_files()[0];
        let mut enc = Encoder::new();
        for record in Decoder::new(file) {
            match record.unwrap() {
                Record::Data(m) if m.global_message_number() == 20 => {
                    let mut bytes = m.bytes().to_vec();
                    let hr = m.field(3).unwrap();
                    bytes[hr.offset()] = 99;
                    match m.compressed_time_offset() {
                        Some(t) => enc.write_compressed_data(m.local_message_type(), t, &bytes),
                        None => enc.write_data(m.local_message_type(), &bytes),
                    }
                    .unwrap();
                }
                r => enc.write_record(&r).unwrap(),
            }
        }
        let out = enc.finish().unwrap();
        let hrs: Vec<_> = Decoder::new(&out)
            .filter_map(|r| match r.unwrap() {
                Record::Data(m) => m.field(3).and_then(|f| f.value()),
                _ => None,
            })
            .collect();
        assert_eq!(hrs, [Value::UInt8(99), Value::UInt8(99)]);
    }

    #[test]
    fn caller_errors() {
        let mut enc = Encoder::new();
        assert_eq!(enc.write_data(0, &[]), Err(EncodeError::NoOpenFile));
        assert_eq!(enc.end_file(), Err(EncodeError::NoOpenFile));
        enc.begin_file(FileOptions::new(1)).unwrap();
        assert_eq!(
            enc.begin_file(FileOptions::new(1)),
            Err(EncodeError::FileAlreadyOpen)
        );
        assert_eq!(
            enc.write_data(2, &[]),
            Err(EncodeError::UndefinedLocalMessage(2))
        );
        assert_eq!(
            enc.write_definition(16, Endian::Little, 0, &[], &[]),
            Err(EncodeError::InvalidLocalMessageType(16))
        );
        let hr = FieldDefinition::new(3, 1, BaseType::UInt8.to_byte());
        enc.write_definition(4, Endian::Little, 20, &[hr], &[])
            .unwrap();
        assert_eq!(
            enc.write_data(4, &[1, 2]),
            Err(EncodeError::MessageSizeMismatch {
                local: 4,
                expected: 1,
                actual: 2
            })
        );
        assert_eq!(
            enc.write_compressed_data(4, 0, &[1]),
            Err(EncodeError::InvalidLocalMessageType(4))
        );
        assert_eq!(
            enc.write_compressed_data(0, 32, &[1]),
            Err(EncodeError::InvalidTimeOffset(32))
        );
        let many = vec![hr; 256];
        assert_eq!(
            enc.write_definition(0, Endian::Little, 20, &many, &[]),
            Err(EncodeError::TooManyFields(256))
        );
    }

    #[test]
    fn empty_file() {
        let mut enc = Encoder::new();
        enc.begin_file(FileOptions::new(2132)).unwrap();
        let bytes = enc.finish().unwrap();
        assert_eq!(bytes, FitBuilder::new().build());
    }

    proptest! {
        /// Any scalar of any base type, written in either byte order,
        /// decodes back to the same bytes and value.
        #[test]
        fn value_round_trip(
            type_index in 0..BaseType::ALL.len(),
            big in any::<bool>(),
            raw in any::<[u8; 8]>(),
        ) {
            let base_type = BaseType::ALL[type_index];
            let size = base_type.size();
            let endian = if big { Endian::Big } else { Endian::Little };
            let bytes = &raw[..size];
            let mut enc = Encoder::new();
            enc.begin_file(FileOptions::new(2132)).unwrap();
            let def = FieldDefinition::new(0, size as u8, base_type.to_byte());
            enc.write_definition(0, endian, 0xFF00, &[def], &[]).unwrap();
            enc.write_data(0, bytes).unwrap();
            let out = enc.finish().unwrap();
            let msg = Decoder::new(&out)
                .find_map(|r| match r.unwrap() {
                    Record::Data(m) => Some(m),
                    _ => None,
                })
                .unwrap();
            let field = msg.field(0).unwrap();
            prop_assert_eq!(field.bytes(), bytes);
            let decoded = field.raw_value();
            let expected = Value::decode(base_type, endian, bytes);
            // Compare bit patterns so NaN payloads count as equal.
            prop_assert_eq!(std::format!("{decoded:?}"), std::format!("{expected:?}"));
        }

        /// Random sequences of definitions and messages re-encode exactly.
        #[test]
        fn random_files_round_trip(
            messages in proptest::collection::vec(
                (0u8..16, proptest::collection::vec(1u8..8, 0..6), any::<bool>(), any::<u8>()),
                0..20,
            ),
        ) {
            let mut b = FitBuilder::new();
            for (local, sizes, big, fill) in &messages {
                let fields: Vec<_> = sizes.iter().enumerate()
                    .map(|(i, &s)| (i as u8, s, 0x0D))
                    .collect();
                b.definition(*local, *big, 20, &fields);
                let total: usize = sizes.iter().map(|&s| usize::from(s)).sum();
                b.data(*local, &vec![*fill; total]);
            }
            let file = b.build();
            prop_assert_eq!(reencode(&file), file);
        }
    }
}
