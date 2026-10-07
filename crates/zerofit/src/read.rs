//! Streaming decoding from any [`std::io::Read`].

use std::io::{self, Read};
use std::vec::Vec;

use crate::crc::Crc16;
use crate::decoder::{DecodeOptions, FILE_CRC_SIZE, LOCAL_TYPES, Record, parse_record, to_offset};
use crate::definition::Definition;
use crate::error::{Error, ErrorKind};
use crate::header::FileHeader;
use crate::record_header::RecordHeader;

/// How many bytes to request from the reader at a time.
const CHUNK: usize = 64 * 1024;
/// Bytes of a definition message up to and including its field count.
const DEFINITION_FIXED: usize = 6;
/// Bytes per field definition.
const FIELD_DEF_SIZE: usize = 3;

/// An error from [`ReadDecoder`]: either the input is malformed, or reading
/// it failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    /// The input is not a valid FIT file.
    #[error(transparent)]
    Decode(#[from] Error),
    /// The underlying reader failed.
    #[error("I/O error at byte offset {offset}: {source}")]
    Io {
        /// Absolute offset of the first byte that could not be read.
        offset: u64,
        /// The reader's error.
        #[source]
        source: io::Error,
    },
}

/// Decodes FIT files from a [`Read`] source with a bounded buffer.
///
/// The streaming counterpart of [`Decoder`](crate::Decoder): it parses
/// records with the same code, so it reports the same errors at the same
/// byte offsets, but it reads the input incrementally. Memory use is one
/// read chunk (64 KiB) plus the largest record, regardless of file size.
///
/// Records borrow the decoder's buffer, so `ReadDecoder` is not an
/// [`Iterator`]; call [`next_record`](Self::next_record) in a loop. Each
/// record is valid until the next call.
///
/// # Validation
///
/// A stream cannot be checked before it is read, so the file CRC is checked
/// when the file *ends*: a file with a bad CRC yields its records and then
/// [`ErrorKind::FileCrcMismatch`]. (The slice decoder checks it up front.)
///
/// # Example
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// # let file = std::io::Cursor::new(bytes);
/// // let file = std::fs::File::open("ride.fit")?;
/// use zerofit::{ReadDecoder, Record};
///
/// let mut decoder = ReadDecoder::new(std::io::BufReader::new(file));
/// let mut messages = 0;
/// while let Some(record) = decoder.next_record() {
///     if let Record::Data(msg) = record? {
///         messages += 1;
///         println!("message {} at {:?}", msg.global_message_number(), msg.timestamp());
///     }
/// }
/// assert_eq!(messages, 1);
/// # Ok::<(), zerofit::ReadError>(())
/// ```
#[derive(Debug)]
pub struct ReadDecoder<R> {
    reader: R,
    options: DecodeOptions,
    /// Buffered input; `buf[start..]` is not yet consumed.
    buf: Vec<u8>,
    start: usize,
    eof: bool,
    /// Absolute offset of `buf[start]`.
    pos: u64,
    state: State,
    /// Bytes left in the current file's data section.
    data_left: u64,
    crc: Crc16,
    definitions: [Option<OwnedDefinition>; LOCAL_TYPES],
    last_timestamp: Option<u32>,
    files: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    FileHeader,
    Records,
    Done,
}

/// A definition copied out of the read buffer.
#[derive(Debug)]
struct OwnedDefinition {
    /// The parsed definition with empty field lists; see [`Self::get`].
    meta: Definition<'static>,
    /// Regular then developer field triples.
    raw: Vec<u8>,
    fields_len: usize,
}

impl OwnedDefinition {
    fn get(&self) -> Definition<'_> {
        let (fields, developer) = self
            .raw
            .split_at_checked(self.fields_len)
            .unwrap_or((&self.raw, &[]));
        self.meta.rebind(fields, developer)
    }
}

impl<R: Read> ReadDecoder<R> {
    /// Creates a decoder reading from `reader` with all validation enabled.
    ///
    /// `ReadDecoder` reads in large chunks, so wrapping `reader` in a
    /// [`BufReader`](std::io::BufReader) is not required.
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self::with_options(reader, DecodeOptions::new())
    }

    /// Creates a decoder reading from `reader` with the given options.
    #[must_use]
    pub fn with_options(reader: R, options: DecodeOptions) -> Self {
        Self {
            reader,
            options,
            buf: Vec::new(),
            start: 0,
            eof: false,
            pos: 0,
            state: State::FileHeader,
            data_left: 0,
            crc: Crc16::new(),
            definitions: core::array::from_fn(|_| None),
            last_timestamp: None,
            files: 0,
        }
    }

    /// Absolute byte offset of the next record to decode.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.pos
    }

    /// Returns the underlying reader.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.reader
    }

    /// Decodes the next record. Returns `None` at the clean end of the input
    /// and after the first error.
    ///
    /// # Errors
    ///
    /// [`ReadError::Decode`] if the input is malformed (with the same kind
    /// and offset as [`Decoder`](crate::Decoder) would report, except for
    /// the timing of CRC errors described above), or [`ReadError::Io`] if
    /// the reader fails.
    pub fn next_record(&mut self) -> Option<Result<Record<'_>, ReadError>> {
        let result = match self.state {
            State::Done => return None,
            State::FileHeader => self.file_header(),
            State::Records if self.data_left == 0 => self.file_end(),
            State::Records => match self.ensure_record() {
                Ok(()) => return self.record(),
                Err(e) => Err(e),
            },
        };
        match result {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => {
                self.state = State::Done;
                None
            }
            Err(e) => {
                self.state = State::Done;
                Some(Err(e))
            }
        }
    }

    fn available(&self) -> &[u8] {
        self.buf.get(self.start..).unwrap_or_default()
    }

    /// Reads until at least `n` unconsumed bytes are buffered or the input
    /// ends.
    fn fill_to(&mut self, n: usize) -> Result<(), ReadError> {
        if self.available().len() >= n || self.eof {
            return Ok(());
        }
        // Drop consumed bytes so the buffer stays bounded.
        self.buf.drain(..self.start);
        self.start = 0;
        while self.buf.len() < n && !self.eof {
            let old = self.buf.len();
            self.buf.resize(old.saturating_add(n.max(CHUNK)), 0);
            let read = match self.buf.get_mut(old..) {
                Some(dst) => self.reader.read(dst),
                None => Ok(0),
            };
            match read {
                Ok(0) => {
                    self.buf.truncate(old);
                    self.eof = true;
                }
                Ok(k) => self.buf.truncate(old.saturating_add(k)),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => self.buf.truncate(old),
                Err(source) => {
                    self.buf.truncate(old);
                    return Err(ReadError::Io {
                        offset: self.pos.saturating_add(to_offset(old)),
                        source,
                    });
                }
            }
        }
        Ok(())
    }

    /// Marks `n` buffered bytes as consumed and feeds them to the CRC.
    fn consume(&mut self, n: usize) {
        let end = self.start.saturating_add(n);
        if let Some(bytes) = self.buf.get(self.start..end) {
            self.crc.update(bytes);
        }
        self.start = end;
        self.pos = self.pos.saturating_add(to_offset(n));
    }

    fn file_header(&mut self) -> Result<Option<Record<'static>>, ReadError> {
        self.fill_to(1)?;
        let Some(&size) = self.available().first() else {
            if self.files > 0 {
                return Ok(None); // clean end after complete files
            }
            return Err(Error::new(ErrorKind::UnexpectedEof { needed: 12 }, self.pos).into());
        };
        self.fill_to(usize::from(size.max(12)).min(14))?;
        let header =
            FileHeader::parse_at(self.available(), self.pos, self.options.checks_header_crc())?;
        self.crc = Crc16::new();
        self.consume(usize::from(header.size()));
        self.data_left = u64::from(header.data_size());
        self.definitions = core::array::from_fn(|_| None);
        self.last_timestamp = None;
        self.files = self.files.saturating_add(1);
        self.state = State::Records;
        Ok(Some(Record::Header(header)))
    }

    fn file_end(&mut self) -> Result<Option<Record<'static>>, ReadError> {
        self.fill_to(FILE_CRC_SIZE)?;
        let at = self.pos;
        let Some(&stored) = self.available().first_chunk::<FILE_CRC_SIZE>() else {
            let needed = FILE_CRC_SIZE.saturating_sub(self.available().len());
            return Err(Error::new(ErrorKind::UnexpectedEof { needed }, at).into());
        };
        let stored = u16::from_le_bytes(stored);
        let computed = self.crc.value();
        if self.options.checks_file_crc() && stored != computed {
            return Err(Error::new(ErrorKind::FileCrcMismatch { stored, computed }, at).into());
        }
        self.consume(FILE_CRC_SIZE);
        self.state = State::FileHeader;
        Ok(Some(Record::FileEnd { crc: stored }))
    }

    /// Buffers the whole next record, if the input has it.
    fn ensure_record(&mut self) -> Result<(), ReadError> {
        loop {
            let needed = record_len(self.available(), |local| {
                self.definitions
                    .get(usize::from(local))
                    .and_then(Option::as_ref)
                    .map(|d| d.meta.message_size())
            });
            if self.available().len() >= needed || self.eof {
                return Ok(());
            }
            self.fill_to(needed)?;
        }
    }

    /// Parses the buffered record. Definitions are copied into owned storage
    /// first, so the returned record can borrow it.
    fn record(&mut self) -> Option<Result<Record<'_>, ReadError>> {
        let is_definition = matches!(
            self.available()
                .first()
                .map(|&b| RecordHeader::from_byte(b)),
            Some(RecordHeader::Definition { .. })
        );
        if is_definition {
            let local = match self.store_definition() {
                Ok(local) => local,
                Err(e) => {
                    self.state = State::Done;
                    return Some(Err(e));
                }
            };
            let definition = self.definitions.get(local)?.as_ref()?.get();
            return Some(Ok(Record::Definition(definition)));
        }

        let rest = self.buf.get(self.start..).unwrap_or_default();
        let definitions = &self.definitions;
        let parsed = match parse_record(
            rest,
            self.pos,
            self.data_left,
            self.last_timestamp,
            |local| {
                definitions
                    .get(usize::from(local))
                    .and_then(Option::as_ref)
                    .map(OwnedDefinition::get)
            },
        ) {
            Ok(parsed) => parsed,
            Err(e) => {
                self.state = State::Done;
                return Some(Err(e.into()));
            }
        };
        // Inline `consume` so the borrow of `buf` held by `parsed` stays
        // disjoint from the fields being updated.
        let end = self.start.saturating_add(parsed.len);
        self.crc.update(rest.get(..parsed.len).unwrap_or_default());
        self.start = end;
        self.pos = self.pos.saturating_add(to_offset(parsed.len));
        self.data_left = self.data_left.saturating_sub(to_offset(parsed.len));
        self.last_timestamp = parsed.last_timestamp;
        Some(Ok(parsed.record))
    }

    /// Parses a definition record, copies it into its local message slot and
    /// returns the slot index.
    fn store_definition(&mut self) -> Result<usize, ReadError> {
        let rest = self.buf.get(self.start..).unwrap_or_default();
        let parsed = parse_record(rest, self.pos, self.data_left, self.last_timestamp, |_| {
            None
        })?;
        let Record::Definition(definition) = parsed.record else {
            return Err(Error::new(ErrorKind::UnexpectedEof { needed: 1 }, self.pos).into());
        };
        let local = usize::from(definition.local_message_type());
        let slot = self
            .definitions
            .get_mut(local)
            .ok_or(Error::new(ErrorKind::UnexpectedEof { needed: 1 }, self.pos))?;
        // Reuse the slot's allocation when redefining a local message type.
        let mut raw = slot.take().map(|d| d.raw).unwrap_or_default();
        raw.clear();
        raw.extend_from_slice(definition.raw_fields());
        let fields_len = raw.len();
        raw.extend_from_slice(definition.raw_developer_fields());
        *slot = Some(OwnedDefinition {
            meta: definition.rebind(&[], &[]),
            raw,
            fields_len,
        });
        let len = parsed.len;
        self.consume(len);
        self.data_left = self.data_left.saturating_sub(to_offset(len));
        Ok(local)
    }
}

/// How many bytes the record at the start of `rest` needs, as far as can be
/// told from the bytes present. May grow as more bytes arrive (a definition's
/// length depends on its field counts). `size_of` gives the message size of a
/// defined local message type; undefined types need only their header byte
/// (parsing then reports the error).
fn record_len(rest: &[u8], size_of: impl Fn(u8) -> Option<usize>) -> usize {
    let Some(&header) = rest.first() else {
        return 1;
    };
    match RecordHeader::from_byte(header) {
        RecordHeader::Definition {
            has_developer_fields,
            ..
        } => {
            let Some(&fields) = rest.get(DEFINITION_FIXED.saturating_sub(1)) else {
                return DEFINITION_FIXED;
            };
            let base =
                DEFINITION_FIXED.saturating_add(usize::from(fields).saturating_mul(FIELD_DEF_SIZE));
            if !has_developer_fields {
                return base;
            }
            let Some(&dev) = rest.get(base) else {
                return base.saturating_add(1);
            };
            base.saturating_add(1)
                .saturating_add(usize::from(dev).saturating_mul(FIELD_DEF_SIZE))
        }
        RecordHeader::Data { local } | RecordHeader::CompressedTimestamp { local, .. } => {
            size_of(local).map_or(1, |size| size.saturating_add(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::Decoder;
    use crate::test_builder::FitBuilder;
    use proptest::prelude::*;
    use std::format;
    use std::string::String;

    /// A reader that returns at most `chunk` bytes per call, and optionally
    /// fails after `fail_at` bytes.
    struct Trickle<'a> {
        data: &'a [u8],
        chunk: usize,
        fail_at: Option<usize>,
        served: usize,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.fail_at.is_some_and(|f| self.served >= f) {
                return Err(io::Error::other("boom"));
            }
            let n = self.chunk.min(buf.len()).min(self.data.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            self.served += n;
            Ok(n)
        }
    }

    /// Renders each decoder's output so the two can be compared.
    fn slice_log(bytes: &[u8], options: DecodeOptions) -> Vec<String> {
        Decoder::with_options(bytes, options)
            .map(|r| match r {
                Ok(r) => format!("{r:?}"),
                Err(e) => format!("ERR {e:?}"),
            })
            .collect()
    }

    fn stream_log(bytes: &[u8], chunk: usize, options: DecodeOptions) -> Vec<String> {
        let reader = Trickle {
            data: bytes,
            chunk,
            fail_at: None,
            served: 0,
        };
        let mut d = ReadDecoder::with_options(reader, options);
        let mut out = Vec::new();
        while let Some(r) = d.next_record() {
            out.push(match r {
                Ok(r) => format!("{r:?}"),
                Err(ReadError::Decode(e)) => format!("ERR {e:?}"),
                Err(e) => format!("IO {e}"),
            });
        }
        out
    }

    fn sample() -> Vec<u8> {
        let mut b = FitBuilder::new();
        b.definition(0, false, 0, &[(0, 1, 0x00), (4, 4, 0x86)])
            .data(0, &[4, 1, 2, 3, 4])
            .definition_with_dev(
                1,
                true,
                20,
                &[(253, 4, 0x86), (3, 1, 2)],
                Some(&[(0, 2, 0)]),
            )
            .data(1, &[0x40, 0, 0, 0, 120, 7, 7])
            .compressed(1, 3, &[0xFF, 0xFF, 0xFF, 0xFF, 121, 8, 8])
            .definition(1, false, 21, &[(0, 2, 0x84)])
            .data(1, &[1, 2]);
        let mut bytes = b.build();
        let second = FitBuilder::new().legacy_header().build();
        bytes.extend_from_slice(&second);
        bytes
    }

    #[test]
    fn matches_slice_decoder_for_every_chunk_size() {
        let bytes = sample();
        let expected = slice_log(&bytes, DecodeOptions::new());
        assert!(expected.iter().all(|l| !l.starts_with("ERR")));
        for chunk in 1..=bytes.len() {
            assert_eq!(stream_log(&bytes, chunk, DecodeOptions::new()), expected);
        }
    }

    #[test]
    fn truncation_errors_match_slice_decoder() {
        let bytes = sample();
        // CRC checks off: the slice decoder checks the CRC of complete files
        // up front, the stream only at the end.
        let options = DecodeOptions::new().validate_file_crc(false);
        for cut in 0..bytes.len() {
            assert_eq!(
                stream_log(&bytes[..cut], 7, options),
                slice_log(&bytes[..cut], options),
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn bad_file_crc_is_reported_at_file_end() {
        let mut bytes = FitBuilder::new().build();
        let n = bytes.len();
        bytes[n - 1] ^= 1;
        let log = stream_log(&bytes, 64, DecodeOptions::new());
        assert_eq!(log.len(), 2);
        assert!(log[1].contains("FileCrcMismatch"), "{log:?}");
        assert_eq!(
            slice_log(&bytes, DecodeOptions::new()).last(),
            log.last(),
            "same kind and offset"
        );
    }

    #[test]
    fn io_errors_carry_the_offset_and_fuse() {
        let bytes = sample();
        let reader = Trickle {
            data: &bytes,
            chunk: 10,
            fail_at: Some(20),
            served: 0,
        };
        let mut d = ReadDecoder::new(reader);
        let err = loop {
            match d.next_record() {
                Some(Ok(_)) => {}
                Some(Err(e)) => break e,
                None => panic!("no error"),
            }
        };
        match err {
            ReadError::Io { offset, .. } => assert_eq!(offset, 20),
            other => panic!("{other:?}"),
        }
        assert!(d.next_record().is_none());
    }

    #[test]
    fn empty_input_is_an_error() {
        assert_eq!(
            stream_log(&[], 1, DecodeOptions::new()),
            slice_log(&[], DecodeOptions::new())
        );
    }

    proptest! {
        #[test]
        fn arbitrary_input_matches_slice_decoder(
            data in proptest::collection::vec(any::<u8>(), 0..300),
            chunk in 1usize..40,
        ) {
            let options = DecodeOptions::new()
                .validate_file_crc(false)
                .validate_header_crc(false);
            prop_assert_eq!(stream_log(&data, chunk, options), slice_log(&data, options));
        }
    }
}
