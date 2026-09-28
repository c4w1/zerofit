//! Error types.
//!
//! Every decoding error carries the absolute byte offset in the input at
//! which the problem was detected, so a corrupt file can be inspected with a
//! hex editor. Errors are plain `Copy` data (no heap, no strings), which keeps
//! them usable in `no_std` builds.

/// A decoding error: what went wrong ([`ErrorKind`]) and where (byte offset).
///
/// ```
/// use zerofit::{Error, ErrorKind};
///
/// let err = Error::new(ErrorKind::UndefinedLocalMessage(3), 120);
/// assert_eq!(err.offset(), 120);
/// assert_eq!(err.kind(), ErrorKind::UndefinedLocalMessage(3));
/// assert_eq!(
///     err.to_string(),
///     "local message type 3 used before being defined at byte offset 120"
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("{kind} at byte offset {offset}")]
pub struct Error {
    offset: u64,
    kind: ErrorKind,
}

impl Error {
    /// Creates an error of `kind` detected at absolute byte `offset`.
    #[must_use]
    pub const fn new(kind: ErrorKind, offset: u64) -> Self {
        Self { offset, kind }
    }

    /// Absolute byte offset in the input where the error was detected.
    ///
    /// For errors about a structure (a header, a record), this is the offset
    /// of the start of that structure.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// The kind of error.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }
}

/// The specific reason decoding failed.
///
/// New variants may be added in minor releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input ended in the middle of a structure.
    #[error("unexpected end of input: {needed} more byte(s) needed")]
    UnexpectedEof {
        /// Minimum number of additional bytes required to make progress.
        needed: usize,
    },

    /// The file header's size byte is not 12 or 14.
    #[error("invalid file header size {0} (expected 12 or 14)")]
    InvalidHeaderSize(u8),

    /// The file header's data type field is not the ASCII signature `.FIT`.
    #[error("invalid file signature {0:02x?} (expected \".FIT\")")]
    InvalidSignature([u8; 4]),

    /// The 14-byte file header's CRC does not match its contents.
    #[error("header CRC mismatch: stored {stored:#06x}, computed {computed:#06x}")]
    HeaderCrcMismatch {
        /// CRC stored in the header.
        stored: u16,
        /// CRC computed over the first 12 header bytes.
        computed: u16,
    },

    /// The CRC at the end of the file does not match its contents.
    #[error("file CRC mismatch: stored {stored:#06x}, computed {computed:#06x}")]
    FileCrcMismatch {
        /// CRC stored at the end of the file.
        stored: u16,
        /// CRC computed over the header and data records.
        computed: u16,
    },

    /// A data message refers to a local message type that has not been
    /// defined by a preceding definition message.
    #[error("local message type {0} used before being defined")]
    UndefinedLocalMessage(u8),

    /// A definition message's architecture byte is neither 0 (little-endian)
    /// nor 1 (big-endian).
    #[error("invalid architecture byte {0} (expected 0 or 1)")]
    InvalidArchitecture(u8),

    /// A record extends past the end of the data section declared in the file
    /// header.
    #[error("record overruns the declared data size by {overrun} byte(s)")]
    DataSizeOverrun {
        /// How many bytes the record extends past the data section.
        overrun: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;

    #[test]
    fn display_includes_kind_and_offset() {
        let err = Error::new(
            ErrorKind::HeaderCrcMismatch {
                stored: 0x1234,
                computed: 0xABCD,
            },
            0,
        );
        assert_eq!(
            err.to_string(),
            "header CRC mismatch: stored 0x1234, computed 0xabcd at byte offset 0"
        );
    }

    #[test]
    fn display_every_kind() {
        let cases = [
            (
                ErrorKind::UnexpectedEof { needed: 3 },
                "unexpected end of input: 3 more byte(s) needed",
            ),
            (
                ErrorKind::InvalidHeaderSize(13),
                "invalid file header size 13 (expected 12 or 14)",
            ),
            (
                ErrorKind::InvalidSignature(*b"XFIT"),
                "invalid file signature [58, 46, 49, 54] (expected \".FIT\")",
            ),
            (
                ErrorKind::FileCrcMismatch {
                    stored: 1,
                    computed: 2,
                },
                "file CRC mismatch: stored 0x0001, computed 0x0002",
            ),
            (
                ErrorKind::UndefinedLocalMessage(7),
                "local message type 7 used before being defined",
            ),
            (
                ErrorKind::InvalidArchitecture(2),
                "invalid architecture byte 2 (expected 0 or 1)",
            ),
            (
                ErrorKind::DataSizeOverrun { overrun: 5 },
                "record overruns the declared data size by 5 byte(s)",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
    }

    #[test]
    fn accessors() {
        let err = Error::new(ErrorKind::UndefinedLocalMessage(3), 1234);
        assert_eq!(err.offset(), 1234);
        assert_eq!(err.kind(), ErrorKind::UndefinedLocalMessage(3));
    }
}
