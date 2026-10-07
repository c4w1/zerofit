//! The zero-copy slice decoder.

use core::iter::FusedIterator;

use crate::base_type::Endian;
use crate::crc::crc16;
use crate::definition::Definition;
use crate::error::{Error, ErrorKind};
use crate::header::FileHeader;
use crate::message::DataMessage;
use crate::record_header::RecordHeader;

/// Number of local message types a normal record header can address.
pub(crate) const LOCAL_TYPES: usize = 16;
/// Size of the CRC that ends every FIT file.
pub(crate) const FILE_CRC_SIZE: usize = 2;
/// Mask for the part of a timestamp a compressed header carries.
const TIME_OFFSET_MASK: u32 = 0x1F;

/// Options controlling validation. The default validates everything.
///
/// ```
/// use zerofit::{DecodeOptions, Decoder};
///
/// // Salvage what we can from a file whose CRCs are known to be wrong.
/// let options = DecodeOptions::new()
///     .validate_header_crc(false)
///     .validate_file_crc(false);
/// let decoder = Decoder::with_options(&[], options);
/// # let _ = decoder;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecodeOptions {
    validate_header_crc: bool,
    validate_file_crc: bool,
}

impl DecodeOptions {
    /// Options with all validation enabled.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            validate_header_crc: true,
            validate_file_crc: true,
        }
    }

    /// Whether to check each 14-byte file header's CRC (default `true`).
    #[must_use]
    pub const fn validate_header_crc(mut self, validate: bool) -> Self {
        self.validate_header_crc = validate;
        self
    }

    /// Whether to check each file's trailing CRC (default `true`).
    #[must_use]
    pub const fn validate_file_crc(mut self, validate: bool) -> Self {
        self.validate_file_crc = validate;
        self
    }
}

impl DecodeOptions {
    pub(crate) const fn checks_header_crc(self) -> bool {
        self.validate_header_crc
    }

    pub(crate) const fn checks_file_crc(self) -> bool {
        self.validate_file_crc
    }
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// One item produced by the [`Decoder`], in file order.
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// use zerofit::{Decoder, Record};
///
/// let kinds: Vec<&str> = Decoder::new(bytes)
///     .map(|r| match r {
///         Ok(Record::Header(_)) => "header",
///         Ok(Record::Definition(_)) => "definition",
///         Ok(Record::Data(_)) => "data",
///         Ok(Record::FileEnd { .. }) => "end",
///         Err(_) => "error",
///     })
///     .collect();
/// assert_eq!(kinds, ["header", "definition", "data", "end"]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record<'a> {
    /// The start of a FIT file.
    Header(FileHeader),
    /// A definition message. It is also applied to the decoder, so you can
    /// ignore these unless you need the layouts themselves.
    Definition(Definition<'a>),
    /// A data message.
    Data(DataMessage<'a>),
    /// The end of a FIT file. Another file may follow in the same input.
    FileEnd {
        /// The stored file CRC.
        crc: u16,
    },
}

/// Decodes FIT files from a byte slice without allocating.
///
/// `Decoder` is an [`Iterator`] over [`Record`]s that borrow from the input.
/// It handles 12- and 14-byte headers, header and file CRCs, per-definition
/// byte order, compressed timestamp headers, developer fields, and several FIT
/// files chained back to back.
///
/// # Validation
///
/// Because the whole file is available, the file CRC is checked *before* the
/// file's header is yielded, so no data from a corrupt file is produced. If
/// the input is too short to contain the CRC, the check cannot run up front
/// and the missing bytes are reported as [`ErrorKind::UnexpectedEof`] when
/// the decoder reaches them. Use [`DecodeOptions`] to turn CRC checks off.
///
/// # Errors
///
/// Decoding stops at the first error: the iterator yields `Some(Err(_))` once
/// and then `None`. Every error carries the byte offset where it occurred.
///
/// # Example
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// use zerofit::{Decoder, Record};
///
/// let mut data_messages = 0;
/// for record in Decoder::new(bytes) {
///     match record? {
///         Record::Header(h) => println!("profile {}", h.profile_version()),
///         Record::Data(msg) => {
///             data_messages += 1;
///             for field in msg.fields() {
///                 println!("{}: {:?}", field.number(), field.value());
///             }
///         }
///         Record::Definition(_) | Record::FileEnd { .. } => {}
///     }
/// }
/// assert_eq!(data_messages, 1);
/// # Ok::<(), zerofit::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    input: &'a [u8],
    pos: usize,
    options: DecodeOptions,
    state: State,
    data_end: usize,
    definitions: [Option<Definition<'a>>; LOCAL_TYPES],
    last_timestamp: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    FileHeader,
    Records,
    Done,
}

impl<'a> Decoder<'a> {
    /// Creates a decoder over `input` with all validation enabled.
    #[must_use]
    pub fn new(input: &'a [u8]) -> Self {
        Self::with_options(input, DecodeOptions::new())
    }

    /// Creates a decoder over `input` with the given options.
    #[must_use]
    pub fn with_options(input: &'a [u8], options: DecodeOptions) -> Self {
        Self {
            input,
            pos: 0,
            options,
            state: State::FileHeader,
            data_end: 0,
            definitions: [None; LOCAL_TYPES],
            last_timestamp: None,
        }
    }

    /// Absolute byte offset of the next record to decode.
    #[must_use]
    pub fn position(&self) -> u64 {
        to_offset(self.pos)
    }

    fn next_record(&mut self) -> Result<Option<Record<'a>>, Error> {
        match self.state {
            State::Done => Ok(None),
            State::FileHeader => self.file_header(),
            State::Records if self.pos >= self.data_end => self.file_end().map(Some),
            State::Records => self.record().map(Some),
        }
    }

    fn file_header(&mut self) -> Result<Option<Record<'a>>, Error> {
        let start = self.pos;
        let rest = self.input.get(start..).unwrap_or_default();
        if rest.is_empty() && start > 0 {
            // Clean end after one or more complete files.
            self.state = State::Done;
            return Ok(None);
        }
        let header =
            FileHeader::parse_at(rest, to_offset(start), self.options.checks_header_crc())?;
        let data_start = start.saturating_add(usize::from(header.size()));
        let data_size = usize::try_from(header.data_size()).unwrap_or(usize::MAX);
        let data_end = data_start.saturating_add(data_size);

        if self.options.checks_file_crc() {
            self.check_file_crc(start, data_end)?;
        }

        self.pos = data_start;
        self.data_end = data_end;
        self.definitions = [None; LOCAL_TYPES];
        self.last_timestamp = None;
        self.state = State::Records;
        Ok(Some(Record::Header(header)))
    }

    /// Checks the CRC of the file spanning `start..data_end` (plus CRC), if
    /// the whole file is present.
    fn check_file_crc(&self, start: usize, data_end: usize) -> Result<(), Error> {
        let crc_end = data_end.saturating_add(FILE_CRC_SIZE);
        let (Some(file), Some(&stored)) = (
            self.input.get(start..data_end),
            self.input
                .get(data_end..crc_end)
                .and_then(|b| b.first_chunk::<FILE_CRC_SIZE>()),
        ) else {
            return Ok(());
        };
        let stored = u16::from_le_bytes(stored);
        let computed = crc16(file);
        if stored == computed {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::FileCrcMismatch { stored, computed },
                to_offset(data_end),
            ))
        }
    }

    fn file_end(&mut self) -> Result<Record<'a>, Error> {
        let start = self.pos;
        let rest = self.input.get(start..).unwrap_or_default();
        let Some(&crc) = rest.first_chunk::<FILE_CRC_SIZE>() else {
            return Err(Error::new(
                ErrorKind::UnexpectedEof {
                    needed: FILE_CRC_SIZE.saturating_sub(rest.len()),
                },
                to_offset(start),
            ));
        };
        self.pos = start.saturating_add(FILE_CRC_SIZE);
        self.state = State::FileHeader;
        Ok(Record::FileEnd {
            crc: u16::from_le_bytes(crc),
        })
    }

    fn record(&mut self) -> Result<Record<'a>, Error> {
        let start = self.pos;
        let rest = self.input.get(start..).unwrap_or_default();
        let data_left = to_offset(self.data_end.saturating_sub(start));
        let definitions = &self.definitions;
        let parsed = parse_record(
            rest,
            to_offset(start),
            data_left,
            self.last_timestamp,
            |local| definitions.get(usize::from(local)).copied().flatten(),
        )?;
        if let Record::Definition(definition) = parsed.record {
            let local = usize::from(definition.local_message_type());
            if let Some(slot) = self.definitions.get_mut(local) {
                *slot = Some(definition);
            }
        }
        self.last_timestamp = parsed.last_timestamp;
        self.pos = start.saturating_add(parsed.len);
        Ok(parsed.record)
    }
}

impl<'a> Iterator for Decoder<'a> {
    type Item = Result<Record<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.next_record() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => None,
            Err(e) => {
                self.state = State::Done;
                Some(Err(e))
            }
        }
    }
}

impl FusedIterator for Decoder<'_> {}

/// One record parsed by [`parse_record`].
pub(crate) struct Parsed<'a> {
    pub(crate) record: Record<'a>,
    /// Bytes consumed, including the record header.
    pub(crate) len: usize,
    /// The most recent full timestamp after this record.
    pub(crate) last_timestamp: Option<u32>,
}

/// Parses the record at the start of `rest`, the core step shared by the
/// slice and streaming decoders.
///
/// `offset` is the absolute offset of `rest`, `data_left` the number of bytes
/// left in the file's data section, `last_timestamp` the most recent full
/// timestamp, and `definition_for` looks up the definition of a local
/// message type. The caller applies the result (stores definitions, advances
/// by `len`, keeps `last_timestamp`).
///
/// If `rest` ends early, the error is [`ErrorKind::UnexpectedEof`] with the
/// number of bytes still needed, so a streaming caller can read more and
/// retry.
pub(crate) fn parse_record<'a>(
    rest: &'a [u8],
    offset: u64,
    data_left: u64,
    last_timestamp: Option<u32>,
    definition_for: impl FnOnce(u8) -> Option<Definition<'a>>,
) -> Result<Parsed<'a>, Error> {
    let Some((&header, body)) = rest.split_first() else {
        return Err(Error::new(ErrorKind::UnexpectedEof { needed: 1 }, offset));
    };
    match RecordHeader::from_byte(header) {
        RecordHeader::Definition {
            local,
            has_developer_fields,
        } => {
            let (definition, consumed) =
                Definition::parse(body, local, has_developer_fields, offset)?;
            let len = consumed.saturating_add(1);
            check_within_data(offset, len, data_left)?;
            Ok(Parsed {
                record: Record::Definition(definition),
                len,
                last_timestamp,
            })
        }
        RecordHeader::Data { local } => data_message(
            body,
            offset,
            data_left,
            last_timestamp,
            local,
            None,
            definition_for,
        ),
        RecordHeader::CompressedTimestamp { local, time_offset } => data_message(
            body,
            offset,
            data_left,
            last_timestamp,
            local,
            Some(time_offset),
            definition_for,
        ),
    }
}

fn data_message<'a>(
    body: &'a [u8],
    offset: u64,
    data_left: u64,
    last_timestamp: Option<u32>,
    local: u8,
    time_offset: Option<u8>,
    definition_for: impl FnOnce(u8) -> Option<Definition<'a>>,
) -> Result<Parsed<'a>, Error> {
    let err = |kind| Error::new(kind, offset);
    let definition = definition_for(local).ok_or(err(ErrorKind::UndefinedLocalMessage(local)))?;
    let size = definition.message_size();
    let len = size.saturating_add(1);
    check_within_data(offset, len, data_left)?;
    let Some(bytes) = body.get(..size) else {
        return Err(err(ErrorKind::UnexpectedEof {
            needed: size.saturating_sub(body.len()),
        }));
    };

    let mut timestamp = match (time_offset, last_timestamp) {
        (Some(t), Some(last)) => Some(expand_timestamp(last, t)),
        _ => None,
    };
    if let Some(full) = read_timestamp(&definition, bytes) {
        timestamp = Some(full);
    }
    Ok(Parsed {
        record: Record::Data(DataMessage::new(
            definition,
            bytes,
            timestamp,
            offset,
            time_offset,
        )),
        len,
        last_timestamp: timestamp.or(last_timestamp),
    })
}

/// Errors if a record of `len` bytes at `offset` extends past the data
/// section, which has `data_left` bytes remaining.
fn check_within_data(offset: u64, len: usize, data_left: u64) -> Result<(), Error> {
    match to_offset(len).checked_sub(data_left) {
        Some(overrun) if overrun > 0 => {
            Err(Error::new(ErrorKind::DataSizeOverrun { overrun }, offset))
        }
        _ => Ok(()),
    }
}

/// Reads a valid 4-byte field 253 from a data message, if the definition has
/// one.
fn read_timestamp(definition: &Definition<'_>, bytes: &[u8]) -> Option<u32> {
    let offset = definition.timestamp_offset()?;
    let &raw = bytes.get(offset..)?.first_chunk::<4>()?;
    let value = match definition.endian() {
        Endian::Little => u32::from_le_bytes(raw),
        Endian::Big => u32::from_be_bytes(raw),
    };
    (value != u32::MAX).then_some(value)
}

/// Reconstructs a full timestamp from the last full timestamp and a 5-bit
/// offset, allowing for one rollover of the low 5 bits.
const fn expand_timestamp(last: u32, offset: u8) -> u32 {
    // `offset` comes from a 5-bit field; masking keeps the invariant local.
    let offset = offset as u32 & TIME_OFFSET_MASK;
    let base = (last & !TIME_OFFSET_MASK) | offset;
    if offset >= last & TIME_OFFSET_MASK {
        base
    } else {
        base.wrapping_add(TIME_OFFSET_MASK + 1)
    }
}

/// Converts a slice position to a stream offset.
pub(crate) fn to_offset(pos: usize) -> u64 {
    u64::try_from(pos).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_builder::{FitBuilder, HeaderCrc};
    use crate::value::Value;
    use proptest::prelude::*;
    use std::vec::Vec;

    const RECORD: u16 = 20;
    const TS: (u8, u8, u8) = (253, 4, 0x86);
    const HR: (u8, u8, u8) = (3, 1, 0x02);

    fn decode_all(bytes: &[u8]) -> Vec<Result<Record<'_>, Error>> {
        Decoder::new(bytes).collect()
    }

    fn ok_records(bytes: &[u8]) -> Vec<Record<'_>> {
        decode_all(bytes).into_iter().map(Result::unwrap).collect()
    }

    fn data<'a>(records: &[Record<'a>]) -> Vec<DataMessage<'a>> {
        records
            .iter()
            .filter_map(|r| match r {
                Record::Data(m) => Some(*m),
                _ => None,
            })
            .collect()
    }

    fn only_error(bytes: &[u8]) -> Error {
        decode_all(bytes)
            .into_iter()
            .find_map(Result::err)
            .expect("expected an error")
    }

    fn ts_hr(ts: u32, hr: u8) -> Vec<u8> {
        let mut p = ts.to_le_bytes().to_vec();
        p.push(hr);
        p
    }

    #[test]
    fn decodes_simple_file() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[TS, HR])
            .data(0, &ts_hr(1000, 120))
            .data(0, &ts_hr(1001, 121));
        let bytes = b.build();
        let records = ok_records(&bytes);
        assert_eq!(records.len(), 5);
        assert!(matches!(records[0], Record::Header(h) if h.size() == 14));
        assert!(matches!(records[1], Record::Definition(d) if d.global_message_number() == RECORD));
        let msgs = data(&records);
        assert_eq!(msgs[0].timestamp(), Some(1000));
        assert_eq!(msgs[1].field(3).unwrap().value(), Some(Value::UInt8(121)));
        assert_eq!(msgs[0].offset(), 14 + 12);
        assert_eq!(msgs[1].offset(), 14 + 12 + 6);
        let crc = u16::from_le_bytes([bytes[bytes.len() - 2], bytes[bytes.len() - 1]]);
        assert_eq!(records[4], Record::FileEnd { crc });
    }

    #[test]
    fn decodes_legacy_header() {
        let mut b = FitBuilder::new().legacy_header();
        b.definition(0, false, RECORD, &[HR]).data(0, &[99]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        assert!(matches!(records[0], Record::Header(h) if h.size() == 12));
        assert_eq!(data(&records)[0].offset(), 12 + 9);
    }

    #[test]
    fn zero_header_crc_is_accepted() {
        let mut b = FitBuilder::new().header_crc(HeaderCrc::Zero);
        b.definition(0, false, RECORD, &[HR]).data(0, &[99]);
        assert_eq!(ok_records(&b.build()).len(), 4);
    }

    #[test]
    fn big_endian_values() {
        let mut b = FitBuilder::new();
        b.definition(1, true, RECORD, &[TS, (7, 2, 0x84)])
            .data(1, &[0, 0, 0x03, 0xE8, 0x12, 0x34]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        let msg = data(&records)[0];
        assert_eq!(msg.timestamp(), Some(1000));
        assert_eq!(msg.field(7).unwrap().value(), Some(Value::UInt16(0x1234)));
        assert_eq!(msg.local_message_type(), 1);
    }

    #[test]
    fn empty_data_section() {
        let bytes = FitBuilder::new().build();
        let records = ok_records(&bytes);
        assert_eq!(records.len(), 2);
        assert!(matches!(records[1], Record::FileEnd { .. }));
    }

    #[test]
    fn interleaved_and_redefined_local_types() {
        let mut b = FitBuilder::new();
        b.definition(0, false, 0, &[(0, 1, 0)])
            .definition(1, false, RECORD, &[HR])
            .data(1, &[60])
            .data(0, &[4])
            // Redefine local 0 with a different layout.
            .definition(0, false, 18, &[(5, 2, 0x84)])
            .data(0, &[0x10, 0x00])
            .data(1, &[61]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        let got: Vec<(u16, Vec<u8>)> = data(&records)
            .iter()
            .map(|m| (m.global_message_number(), m.bytes().to_vec()))
            .collect();
        assert_eq!(
            got,
            [
                (RECORD, std::vec![60]),
                (0, std::vec![4]),
                (18, std::vec![0x10, 0]),
                (RECORD, std::vec![61]),
            ]
        );
    }

    #[test]
    fn undefined_local_message_is_error_and_fuses() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[HR]).data(3, &[1]);
        let bytes = b.build();
        let mut dec = Decoder::new(&bytes);
        assert!(dec.next().unwrap().is_ok()); // header
        assert!(dec.next().unwrap().is_ok()); // definition
        let err = dec.next().unwrap().unwrap_err();
        assert_eq!(err.kind(), ErrorKind::UndefinedLocalMessage(3));
        assert_eq!(err.offset(), 14 + 9);
        assert!(dec.next().is_none());
        assert!(dec.next().is_none());
    }

    #[test]
    fn compressed_timestamps_and_rollover() {
        let base = 0x1000_001Eu32; // low 5 bits = 30
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[TS, HR])
            .definition(1, false, RECORD, &[HR])
            .data(0, &ts_hr(base, 100))
            .compressed(1, 31, &[101]) // same 32 s window: base + 1
            .compressed(1, 2, &[102]) // rolled over: base + 4
            .compressed(1, 2, &[103]) // same offset again: no change
            .compressed(1, 10, &[104]); // base + 12
        let bytes = b.build();
        let records = ok_records(&bytes);
        let ts: Vec<_> = data(&records).iter().map(DataMessage::timestamp).collect();
        assert_eq!(
            ts,
            [
                Some(base),
                Some(base + 1),
                Some(base + 4),
                Some(base + 4),
                Some(base + 12)
            ]
        );
        assert!(data(&records)[1].has_compressed_timestamp());
        assert!(!data(&records)[0].has_compressed_timestamp());
    }

    #[test]
    fn compressed_timestamp_without_reference_is_none() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[HR]).compressed(0, 5, &[1]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        assert_eq!(data(&records)[0].timestamp(), None);
    }

    #[test]
    fn invalid_timestamp_field_is_ignored() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[TS, HR])
            .definition(1, false, RECORD, &[HR])
            .data(0, &ts_hr(64, 1))
            .data(0, &ts_hr(u32::MAX, 2))
            .compressed(1, 1, &[3]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        let ts: Vec<_> = data(&records).iter().map(DataMessage::timestamp).collect();
        assert_eq!(ts, [Some(64), None, Some(65)]);
    }

    #[test]
    fn expand_timestamp_cases() {
        assert_eq!(expand_timestamp(0, 0), 0);
        assert_eq!(expand_timestamp(0x20, 0x1F), 0x3F);
        assert_eq!(expand_timestamp(0x3F, 0x00), 0x40);
        assert_eq!(expand_timestamp(u32::MAX, 0), 0); // wraps rather than panics
    }

    #[test]
    fn developer_fields() {
        let mut b = FitBuilder::new();
        b.definition_with_dev(0, false, RECORD, &[HR], Some(&[(0, 2, 0), (1, 1, 1)]))
            .data(0, &[70, 0xAA, 0xBB, 0xCC]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        let msg = data(&records)[0];
        assert_eq!(msg.fields().count(), 1);
        let dev: Vec<_> = msg.developer_fields().collect();
        assert_eq!(dev.len(), 2);
        assert_eq!(dev[0].bytes(), [0xAA, 0xBB]);
        assert_eq!(dev[1].bytes(), [0xCC]);
        assert_eq!(dev[1].number(), 1);
        assert_eq!(dev[1].developer_data_index(), 1);
    }

    #[test]
    fn field_access_and_invalid_values() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[HR, (4, 1, 0x02), (9, 3, 0x07)])
            .data(0, &[0xFF, 7, b'h', b'i', 0]);
        let bytes = b.build();
        let records = ok_records(&bytes);
        let msg = data(&records)[0];
        let hr = msg.field(3).unwrap();
        assert_eq!(hr.value(), None);
        assert_eq!(hr.raw_value(), Value::UInt8(0xFF));
        assert_eq!(hr.bytes(), [0xFF]);
        assert_eq!(hr.definition().size(), 1);
        assert_eq!(msg.field(4).unwrap().value(), Some(Value::UInt8(7)));
        let Some(Value::String(s)) = msg.field(9).unwrap().value() else {
            panic!()
        };
        assert_eq!(s.to_str(), Ok("hi"));
        assert!(msg.field(100).is_none());
        assert_eq!(msg.definition().field_count(), 3);
    }

    #[test]
    fn file_crc_mismatch_is_reported_before_any_record() {
        let mut b = FitBuilder::new().file_crc(0x1234);
        b.definition(0, false, RECORD, &[HR]).data(0, &[1]);
        let bytes = b.build();
        let all = decode_all(&bytes);
        assert_eq!(all.len(), 1);
        let err = all[0].unwrap_err();
        assert!(matches!(
            err.kind(),
            ErrorKind::FileCrcMismatch { stored: 0x1234, .. }
        ));
        assert_eq!(err.offset(), bytes.len() as u64 - 2);
    }

    #[test]
    fn file_crc_check_can_be_disabled() {
        let mut b = FitBuilder::new().file_crc(0x1234);
        b.definition(0, false, RECORD, &[HR]).data(0, &[1]);
        let bytes = b.build();
        let options = DecodeOptions::new().validate_file_crc(false);
        let records: Vec<_> = Decoder::with_options(&bytes, options)
            .map(Result::unwrap)
            .collect();
        assert_eq!(records.last(), Some(&Record::FileEnd { crc: 0x1234 }));
    }

    #[test]
    fn header_crc_mismatch_and_option() {
        let bytes = FitBuilder::new()
            .header_crc(HeaderCrc::Value(0xBEEF))
            .build();
        assert!(matches!(
            only_error(&bytes).kind(),
            ErrorKind::HeaderCrcMismatch { stored: 0xBEEF, .. }
        ));
        let options = DecodeOptions::default().validate_header_crc(false);
        assert!(Decoder::with_options(&bytes, options).all(|r| r.is_ok()));
    }

    #[test]
    fn data_size_too_small_is_overrun() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[TS, HR])
            .data(0, &ts_hr(1, 2));
        let declared = b.records_len() - 3;
        let bytes = b.data_size(declared as u32).build();
        let options = DecodeOptions::new().validate_file_crc(false);
        let err = Decoder::with_options(&bytes, options)
            .find_map(Result::err)
            .unwrap();
        assert_eq!(err.kind(), ErrorKind::DataSizeOverrun { overrun: 3 });
        assert_eq!(err.offset(), 14 + 12);
    }

    #[test]
    fn definition_overrunning_data_size() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[TS, HR]);
        let bytes = b.data_size(8).build();
        let options = DecodeOptions::new().validate_file_crc(false);
        let err = Decoder::with_options(&bytes, options)
            .find_map(Result::err)
            .unwrap();
        assert_eq!(err.kind(), ErrorKind::DataSizeOverrun { overrun: 4 });
        assert_eq!(err.offset(), 14);
    }

    #[test]
    fn every_truncation_ends_in_eof() {
        let mut b = FitBuilder::new();
        b.definition_with_dev(0, false, RECORD, &[TS, HR], Some(&[(0, 1, 0)]))
            .data(0, &[1, 0, 0, 0, 2, 3])
            .compressed(0, 4, &[1, 0, 0, 0, 2, 3]);
        let bytes = b.build();
        for len in 0..bytes.len() {
            let all = decode_all(&bytes[..len]);
            let last = all.last().unwrap();
            assert!(
                matches!(last, Err(e) if matches!(e.kind(), ErrorKind::UnexpectedEof { needed } if needed > 0)),
                "len {len}: {last:?}"
            );
            assert!(all[..all.len() - 1].iter().all(Result::is_ok));
        }
    }

    #[test]
    fn empty_input_is_eof() {
        assert_eq!(
            only_error(&[]).kind(),
            ErrorKind::UnexpectedEof { needed: 12 }
        );
    }

    #[test]
    fn chained_files_reset_definitions() {
        let mut first = FitBuilder::new();
        first.definition(0, false, RECORD, &[HR]).data(0, &[1]);
        let mut second = FitBuilder::new().legacy_header();
        second.definition(0, false, 0, &[(0, 1, 0)]).data(0, &[4]);
        let mut bytes = first.build();
        let second_start = bytes.len();
        bytes.extend(second.build());

        let records = ok_records(&bytes);
        let headers = records
            .iter()
            .filter(|r| matches!(r, Record::Header(_)))
            .count();
        let ends = records
            .iter()
            .filter(|r| matches!(r, Record::FileEnd { .. }))
            .count();
        assert_eq!((headers, ends), (2, 2));
        let globals: Vec<_> = data(&records)
            .iter()
            .map(DataMessage::global_message_number)
            .collect();
        assert_eq!(globals, [RECORD, 0]);
        assert_eq!(data(&records)[1].offset(), (second_start + 12 + 9) as u64);

        // Local type 0 from the first file must not leak into the second.
        let mut third = FitBuilder::new();
        third.data(0, &[1]);
        let mut bytes = first.build();
        let third_start = bytes.len();
        bytes.extend(third.build());
        let err = only_error(&bytes);
        assert_eq!(err.kind(), ErrorKind::UndefinedLocalMessage(0));
        assert_eq!(err.offset(), (third_start + 14) as u64);
    }

    #[test]
    fn trailing_garbage_is_an_error() {
        let mut bytes = FitBuilder::new().build();
        let end = bytes.len();
        bytes.extend_from_slice(&[0x00, 0x01]);
        let err = only_error(&bytes);
        assert_eq!(err.kind(), ErrorKind::InvalidHeaderSize(0));
        assert_eq!(err.offset(), end as u64);
    }

    #[test]
    fn crc_is_not_checked_up_front_when_file_is_truncated() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[HR]).data(0, &[1]);
        let bytes = b.build();
        let truncated = &bytes[..bytes.len() - 1];
        let all = decode_all(truncated);
        assert_eq!(all.iter().filter(|r| r.is_ok()).count(), 3);
        assert_eq!(
            all.last().unwrap().unwrap_err().kind(),
            ErrorKind::UnexpectedEof { needed: 1 }
        );
    }

    #[test]
    fn position_advances() {
        let mut b = FitBuilder::new();
        b.definition(0, false, RECORD, &[HR]).data(0, &[1]);
        let bytes = b.build();
        let mut dec = Decoder::new(&bytes);
        let mut positions = std::vec![dec.position()];
        while let Some(r) = dec.next() {
            r.unwrap();
            positions.push(dec.position());
        }
        assert_eq!(positions, [0, 14, 23, 25, 27]);
    }

    /// A generated message layout: (field number, size, base type byte).
    fn layout() -> impl Strategy<Value = (bool, u16, Vec<(u8, u8, u8)>)> {
        let field = (0u8..=252, 0u8..=16, 0u8..=0x1F).prop_map(|(n, s, t)| (n, s, t));
        (
            any::<bool>(),
            any::<u16>(),
            proptest::collection::vec(field, 0..8),
        )
    }

    proptest! {
        /// Whatever the builder writes, the decoder reads back.
        #[test]
        fn builder_round_trip(
            layouts in proptest::collection::vec(layout(), 1..4),
            messages in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..20),
        ) {
            let mut b = FitBuilder::new();
            for (local, (be, global, fields)) in layouts.iter().enumerate() {
                b.definition(local as u8, *be, *global, fields);
            }
            let mut expected = Vec::new();
            for (which, fill) in &messages {
                let local = which.index(layouts.len());
                let (_, global, fields) = &layouts[local];
                let size: usize = fields.iter().map(|f| usize::from(f.1)).sum();
                let payload: Vec<u8> = (0..size).map(|i| fill.wrapping_add(i as u8)).collect();
                b.data(local as u8, &payload);
                expected.push((*global, local as u8, payload));
            }
            let bytes = b.build();
            let records: Vec<_> = Decoder::new(&bytes).collect::<Result<_, _>>().unwrap();
            let got: Vec<_> = data(&records)
                .iter()
                .map(|m| (m.global_message_number(), m.local_message_type(), m.bytes().to_vec()))
                .collect();
            prop_assert_eq!(got, expected);
            for m in data(&records) {
                let sizes: usize = m.fields().map(|f| f.bytes().len()).sum();
                prop_assert_eq!(sizes, m.bytes().len());
            }
        }

        /// Arbitrary bytes never cause a panic, and decoding always makes
        /// progress and terminates.
        #[test]
        fn arbitrary_input_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
            exercise(&bytes);
        }

        /// Same, but starting from a valid header so the record paths are hit.
        #[test]
        fn arbitrary_records_never_panic(records in proptest::collection::vec(any::<u8>(), 0..512)) {
            let mut b = FitBuilder::new();
            b.raw(&records);
            let options = DecodeOptions::new().validate_file_crc(false);
            let bytes = b.build();
            let mut dec = Decoder::with_options(&bytes, options);
            let mut last = dec.position();
            while let Some(r) = dec.next() {
                if let Ok(Record::Data(m)) = r {
                    touch(&m);
                }
                if r.is_ok() {
                    prop_assert!(dec.position() > last);
                }
                last = dec.position();
            }
        }
    }

    fn exercise(bytes: &[u8]) {
        let mut dec = Decoder::new(bytes);
        let mut last = dec.position();
        let mut steps = 0;
        while let Some(r) = dec.next() {
            if let Ok(Record::Data(m)) = r {
                touch(&m);
            }
            if r.is_ok() {
                assert!(dec.position() > last);
            }
            last = dec.position();
            steps += 1;
            assert!(steps <= bytes.len() + 1);
        }
    }

    fn touch(m: &DataMessage<'_>) {
        for f in m.fields() {
            let v = f.raw_value();
            let _ = v.is_invalid();
            let _ = v.as_f64();
        }
        for d in m.developer_fields() {
            let _ = d.bytes();
        }
        let _ = m.timestamp();
    }
}
