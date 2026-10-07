//! Differential fuzzing: the streaming `ReadDecoder` must produce exactly
//! the records and errors of the slice `Decoder`, whatever chunk sizes the
//! reader returns.
//!
//! The first input byte picks the reader's chunk size; the rest is the file.
//! CRC validation is off because the two decoders deliberately check the
//! file CRC at different times (up front vs at file end).

#![no_main]

use std::io::{self, Read};

use libfuzzer_sys::fuzz_target;
use zerofit::{DecodeOptions, Decoder, ReadDecoder, ReadError};

/// Returns at most `chunk` bytes per read.
struct Chunked<'a> {
    data: &'a [u8],
    chunk: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.chunk.min(buf.len()).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fuzz_target!(|input: &[u8]| {
    let Some((&chunk, data)) = input.split_first() else {
        return;
    };
    let options = DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false);

    let slice: Vec<String> = Decoder::with_options(data, options)
        .map(|r| format!("{r:?}"))
        .collect();

    let reader = Chunked {
        data,
        chunk: usize::from(chunk % 64) + 1,
    };
    let mut stream = Vec::new();
    let mut decoder = ReadDecoder::with_options(reader, options);
    while let Some(r) = decoder.next_record() {
        stream.push(match r {
            Ok(record) => format!("{:?}", Ok::<_, zerofit::Error>(record)),
            Err(ReadError::Decode(e)) => format!("{:?}", Err::<(), _>(e)),
            Err(e) => panic!("I/O error from an in-memory reader: {e}"),
        });
    }
    assert_eq!(slice, stream);
});
