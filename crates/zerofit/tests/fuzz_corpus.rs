//! Replays the minimized fuzz corpus (`tests/fuzz_corpus/`) and every input
//! that ever crashed a fuzz target through the fuzz targets' invariants, so
//! `cargo test` keeps checking what fuzzing explored:
//!
//! - decoding never panics, always advances, reports offsets inside the
//!   input, and stops after the first error (the `decode` target);
//! - the streaming decoder matches the slice decoder at several chunk sizes
//!   (the `differential` target);
//! - decoded records re-encode to a file that decodes, CRCs checked, to the
//!   same definitions and data (the `roundtrip` target).
//!
//! Regenerate the corpus after a long fuzzing run with `cargo fuzz cmin`
//! (see `fuzz/README.md`).

#![cfg(feature = "std")]
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::io::{self, Read};
use std::path::Path;

use zerofit::encode::Encoder;
use zerofit::{DecodeOptions, Decoder, ReadDecoder, ReadError, Record};

fn inputs() -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz_corpus");
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .map(|d| {
            d.map(|e| e.unwrap().path())
                .filter(|p| p.is_file())
                .map(|p| {
                    let name = p.file_name().unwrap().to_string_lossy().into_owned();
                    (name, std::fs::read(&p).unwrap())
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    // The empty input once crashed the roundtrip target's harness.
    out.push(("<empty>".into(), Vec::new()));
    out
}

fn lenient() -> DecodeOptions {
    DecodeOptions::new()
        .validate_header_crc(false)
        .validate_file_crc(false)
}

#[test]
fn decode_invariants() {
    for (name, data) in inputs() {
        for options in [DecodeOptions::new(), lenient()] {
            let mut decoder = Decoder::with_options(&data, options);
            let mut position = decoder.position();
            while let Some(record) = decoder.next() {
                match record {
                    Ok(record) => {
                        assert!(decoder.position() > position, "{name}: no progress");
                        position = decoder.position();
                        if let Record::Data(m) = record {
                            for f in m.fields() {
                                let _ = f.raw_value().as_f64();
                            }
                        }
                    }
                    Err(e) => {
                        assert!(e.offset() <= data.len() as u64, "{name}: {e:?}");
                        assert!(decoder.next().is_none(), "{name}: not fused");
                        break;
                    }
                }
            }
        }
    }
}

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

#[test]
fn streaming_matches_slice() {
    for (name, data) in inputs() {
        let slice: Vec<String> = Decoder::with_options(&data, lenient())
            .map(|r| format!("{r:?}"))
            .collect();
        for chunk in [3, 4096] {
            let mut d = ReadDecoder::with_options(Chunked { data: &data, chunk }, lenient());
            let mut stream = Vec::new();
            while let Some(r) = d.next_record() {
                stream.push(match r {
                    Ok(r) => format!("{:?}", Ok::<_, zerofit::Error>(r)),
                    Err(ReadError::Decode(e)) => format!("{:?}", Err::<(), _>(e)),
                    Err(e) => panic!("{name}: {e}"),
                });
            }
            assert_eq!(slice, stream, "{name} with chunk {chunk}");
        }
    }
}

#[test]
fn reencoding_round_trips() {
    for (name, data) in inputs() {
        let mut original = Vec::new();
        let mut enc = Encoder::new();
        for record in Decoder::with_options(&data, lenient()) {
            let Ok(record) = record else { break };
            enc.write_record(&record).unwrap();
            if matches!(record, Record::Definition(_) | Record::Data(_)) {
                original.push(format!("{record:?}"));
            }
        }
        let encoded = enc.finish().unwrap();
        if encoded.is_empty() {
            continue;
        }
        let again: Vec<String> = Decoder::new(&encoded)
            .map(|r| r.unwrap_or_else(|e| panic!("{name}: {e}")))
            .filter(|r| matches!(r, Record::Definition(_) | Record::Data(_)))
            .map(|r| format!("{r:?}"))
            .collect();
        assert_eq!(original, again, "{name}");
    }
}
