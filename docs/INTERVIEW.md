# zerofit: interview guide

A walkthrough of this codebase for someone who has to explain and defend it.
Every claim points at a file and function so you can open it and check. Read
it with the code open; §4 is the one to know cold.

Contents:

1. [What it is, in one minute](#1-what-it-is-in-one-minute)
2. [The FIT format in plain English](#2-the-fit-format-in-plain-english)
3. [Architecture and why](#3-architecture-and-why)
4. [How the decoder works, step by step](#4-how-the-decoder-works-step-by-step)
5. [The five design decisions that matter](#5-the-five-design-decisions-that-matter)
6. [Benchmarks: what they show and why we are faster](#6-benchmarks-what-they-show-and-why-we-are-faster)
7. [How correctness is verified](#7-how-correctness-is-verified)
8. [The hardest bugs, and what found them](#8-the-hardest-bugs-and-what-found-them)
9. [15 likely interview questions](#9-15-likely-interview-questions)
10. [zerofit-analytics: training metrics on top of the decoder](#10-zerofit-analytics-training-metrics-on-top-of-the-decoder)

---

## 1. What it is, in one minute

`zerofit` is a Rust library that reads FIT files, the binary activity files
that Garmin, Wahoo and other devices record. It is built for the constraints
of embedded systems: it runs without the standard library (`no_std`),
performs no heap allocation, never copies the input, and cannot panic on
malformed data; every problem becomes an error value carrying the exact byte
offset.

It has two layers:

- **`zerofit`** (`crates/zerofit/`): the *protocol*. File headers, CRCs,
  record headers, definition and data messages, the 17 base types. It knows
  nothing about what "heart rate" is. It also has a streaming decoder for
  `std::io::Read` and a minimal encoder.
- **`zerofit-profile`** (`crates/zerofit-profile/`): the *profile*. Generated
  from Garmin's `Profile.xlsx`, it turns "message 20, field 3" into
  `record.heart_rate()` and applies scales and offsets.

Correctness is checked against Garmin's own FitCSVTool on real recordings,
with 96 generated corruption cases and three fuzz targets.
It is 20-28x faster than the `fitparser` crate at fully decoding a file
and performs zero heap allocations (fitparser: up to 1.6 million).

## 2. The FIT format in plain English

A FIT file is a short header, a stream of records, and a 2-byte checksum.

```
+-------------+----------------------------------------+---------+
| file header | record record record ... record        | CRC-16  |
| 12/14 bytes | (data_size bytes)                      | 2 bytes |
+-------------+----------------------------------------+---------+
```

**The header** (`crates/zerofit/src/header.rs`, see the table in the module
docs): its own size (12, or 14 with a header CRC), a protocol version, the
profile version, the number of record bytes that follow, the ASCII text
`.FIT`, and optionally a CRC of the first 12 bytes.

**Records.** Every record starts with a 1-byte header
(`crates/zerofit/src/record_header.rs`). There are two kinds:

- A **definition message** says: "from now on, local message type 3 means
  global message 20 (`record`), stored big-endian, and contains: field 253,
  4 bytes, uint32; field 3, 1 byte, uint8; ...". It is the schema.
- A **data message** says only "local message type 3" and then the raw bytes
  for each field, laid out exactly as the current definition for type 3
  describes. There are no field tags and no lengths in a data message; you
  cannot even tell where it ends without its definition.

Local message types are slots 0-15. A file defines a slot, sends many data
messages through it, and may redefine the slot later with a different
layout. That is why a decoder must keep a table of the 16 current
definitions.

**Compressed timestamp headers.** To save space, a data message's header
byte can instead carry a 5-bit time offset (bits 0-4) and a 2-bit local type
(slots 0-3 only). The message's timestamp is the last full timestamp seen,
with its low 5 bits replaced, plus 32 seconds if that went backwards
(rollover). See `expand_timestamp` in `crates/zerofit/src/decoder.rs`.

**Base types.** Every field has one of 17 base types (uint8, sint16, string,
float32, ...; table in `crates/zerofit/src/base_type.rs`). Each has an
**invalid sentinel** meaning "no value": `0xFF` for uint8, `0x7FFF` for
sint16, `0` for the `z` types. A device writes the sentinel when, say, the
heart-rate strap is not connected. A field whose size is a multiple of the
type's size is an **array**.

**The profile** is the dictionary that gives numbers meaning: global message
20 is `record`, its field 3 is `heart_rate` in bpm, field 6 is `speed`
stored as m/s × 1000, field 2 is `altitude` stored as (m + 500) × 5. It lives
in the FIT SDK as `Profile.xlsx`, not in the file.

**Developer fields** let apps (Connect IQ data fields, power-meter apps) add
their own fields. A `field_description` message (global 206) in the file
declares "developer app 0, field 5 is called Glucose, uint16, scale 10,
units mg/dL", and later definitions append developer field triples after the
normal ones.

**Chaining.** Several complete FIT files (header, records, CRC) can be
concatenated in one stream; the definitions reset at each header.

**CRC-16.** The checksum is CRC-16/ARC (polynomial 0xA001 reflected). A
property worth knowing: appending a message's CRC to it makes the CRC of the
whole thing zero.

## 3. Architecture and why

```
crates/zerofit/          protocol: Decoder, ReadDecoder, Encoder (no_std core)
crates/zerofit-profile/  profile: generated typed views + developer fields
crates/zerofit-codegen/  Profile.xlsx -> profile-subset.json -> Rust (not published)
crates/zerofit-bench/    criterion + allocation counts vs fitparser (not published)
xtask/                   cargo xtask codegen | extract | expected
fuzz/                    cargo-fuzz targets: decode, differential, roundtrip
```

**Why two layers.** The protocol is small, stable and security-relevant
(it parses untrusted bytes). The profile is large (thousands of fields),
changes with every SDK release, and is licensed differently (derived from
Garmin's spreadsheet). Separating them means the part that must be
panic-free and fuzzed is about 3,300 lines including docs, the generated part can be
regenerated without touching it, and users who only need raw fields do not
compile the profile. The raw layer never names a message or field, with one
exception: field 253 (`timestamp`), because compressed timestamp headers are
part of the protocol and depend on it (`read_timestamp` in
`crates/zerofit/src/decoder.rs`).

**Why the profile is generated and checked in.** Hand-writing 400+ accessors
invites typos in scales; generating at build time would need `Profile.xlsx`
on every build machine, and it cannot be redistributed. So
`zerofit-codegen` runs in two steps (`crates/zerofit-codegen/src/lib.rs`):
`extract` (needs the SDK) writes a trimmed `profile-subset.json`, and
`generate` (needs nothing) writes Rust. Both artefacts are committed; CI
runs `cargo xtask codegen --check` (`xtask/src/main.rs`, `codegen`) to prove
the Rust matches the subset.

**Why a separate xtask.** Ground truth (`cargo xtask expected`) must be
independent of the code under test, so it lives in a crate that is
forbidden from depending on `zerofit` (enforced by the test
`does_not_depend_on_our_decoder` in `xtask/src/main.rs`).

## 4. How the decoder works, step by step

Follow `Decoder` in `crates/zerofit/src/decoder.rs`.

1. **Construction** (`Decoder::with_options`). Stores the input slice, the
   options, position 0, state `FileHeader`, and a table
   `definitions: [Option<Definition<'a>>; 16]`, all of it on the stack. No
   work yet.

2. **`next()`** (the `Iterator` impl) calls `next_record` and, on any error,
   sets the state to `Done` so the iterator is *fused*: after one
   `Some(Err(_))` it returns `None` forever.

3. **File header** (`file_header`). Parses the header with
   `FileHeader::parse_at` (`header.rs`), which checks the size byte (12 or
   14), the `.FIT` signature and, if present and non-zero, the header CRC.
   Then, because the whole file is in memory, **`check_file_crc` verifies the
   file CRC before yielding anything**: a corrupt file produces no data. It
   resets the 16 definition slots and the last timestamp (for chained files)
   and computes `data_end`.

4. **Records** (`record`). While `pos < data_end`, it calls
   `parse_record(rest, offset, data_left, last_timestamp, lookup)`. This
   free function is the heart of the crate and is shared with the streaming
   decoder:
   - reads the record header byte (`RecordHeader::from_byte`, a pure bit
     decode where every byte value is valid);
   - **definition**: `Definition::parse` (`definition.rs`) reads the 5 fixed
     bytes (architecture, global number, field count), then *borrows* the
     field triples as `&'a [u8]` without copying them, and precomputes the
     message size and the offset of a 4-byte field 253. `check_within_data`
     makes sure the record does not run past the declared data size;
   - **data message** (`data_message`): looks up the definition for the
     local type (error `UndefinedLocalMessage` if empty), checks the size
     against the data section, takes `body.get(..size)` (error
     `UnexpectedEof { needed }` if short), and works out the timestamp: the
     message's own field 253 if valid, else the compressed offset expanded
     against the last timestamp.
   - It returns a `Parsed { record, len, last_timestamp }`; the caller
     stores definitions in the slot, advances by `len` and keeps the new
     timestamp.

5. **File end** (`file_end`). At `data_end`, reads the 2-byte CRC (already
   verified) and yields `Record::FileEnd`. The state goes back to
   `FileHeader`; if bytes remain, a chained file follows.

6. **Lazy field decoding.** A `DataMessage<'a>` (`message.rs`) is just a
   copy of its `Definition` (two slices and some integers), the message's
   byte slice, and the timestamp. Nothing has been decoded. `msg.fields()`
   walks the definition triples and splits the bytes with
   `split_at_checked`; `field.value()` calls `Value::decode` (`value.rs`),
   which reads the bytes as the base type in the definition's byte order and
   returns `None` for the invalid sentinel. Arrays are a `Value::Array`
   that decodes elements one at a time; strings are a `FitStr` borrowing
   up to the first NUL.

7. **Profile layer** (`crates/zerofit-profile/src/generated/messages.rs`).
   `Record::new(msg)` checks the global number and wraps the message;
   `record.speed()` is `read::scaled(&self.0, 6, 1000.0, 0.0)`, which finds
   field 6, converts whatever integer type the device actually used to
   `f64`, and returns `raw / scale - offset` (`read` module in
   `crates/zerofit-profile/src/lib.rs`).

**The streaming decoder** (`crates/zerofit/src/read.rs`, `ReadDecoder`) does
the same with a buffer:

- `ensure_record` asks `record_len` how many bytes the next record needs
  (a cheap look at the header byte, the field counts, or the definition's
  message size) and calls `fill_to` until the buffer has them or the input
  ends. `fill_to` compacts consumed bytes and reads in 64 KiB chunks.
- It then calls the **same** `parse_record`. If the input ended early,
  `parse_record` produces exactly the `UnexpectedEof` the slice decoder
  would.
- Definitions cannot borrow the buffer (it gets overwritten), so
  `store_definition` copies the 3-byte triples into an owned slot
  (`OwnedDefinition`, reusing its `Vec`) and `Definition::rebind` makes a
  `Definition` that borrows the slot instead.
- The file CRC is accumulated incrementally (`consume`) and checked at the
  end of the file, since a stream cannot be checked before it is read.
- Records borrow the buffer, so `ReadDecoder` is not an `Iterator`: you call
  `next_record()` in a `while let` loop (a "lending iterator").

## 5. The five design decisions that matter

### 5.1 Zero-copy

**Decision.** Nothing is copied out of the input. `Definition<'a>` holds the
field list as a borrowed `&'a [u8]` of 3-byte triples
(`crates/zerofit/src/definition.rs`), which makes it `Copy`, so
`DataMessage<'a>` can carry its own copy of the definition and the slice
decoder can be a plain `Iterator` whose items borrow only the input.

**Why.** Allocation is the dominant cost of FIT decoding in other
libraries (see §6), and on a microcontroller there may be no allocator at
all. Borrowing also makes "lazy" decoding free: an unread field costs
nothing.

**Tradeoffs.** The whole file must be in one buffer for the zero-allocation
API (solved for large files by `ReadDecoder`, which does allocate a bounded
buffer). Lifetimes leak into the API: a `DataMessage<'a>` cannot outlive the
buffer, and the streaming decoder cannot be an `Iterator`. `Definition` is
copied into each `DataMessage` (about 64 bytes) rather than referenced, a
deliberate trade of a small memcpy for a simpler, borrow-free iterator.

### 5.2 `no_std`

**Decision.** The core is `#![no_std]` with optional `alloc` and `std`
features (`crates/zerofit/src/lib.rs`, `Cargo.toml` `[features]`). CI builds
it for `thumbv7em-none-eabihf` (a Cortex-M4F).

**Why.** FIT files are produced on embedded devices; a library that can run
on the device (or on a sensor hub that inspects files) is the distinctive
selling point, and the constraint forces a cleaner design (no hidden
allocation, plain-data errors).

**Tradeoffs.** No `HashMap` (fixed 16-slot table; developer fields use
`BTreeMap` behind `alloc`), no `io::Error` in the core error, no `f64::round`
in `core` (profile scaling uses only `/` and `-`), and every feature
combination must be tested (CI runs tests with and without default
features and checks MSRV for both).

### 5.3 Code generation for the profile

**Decision.** Typed accessors, enums and field metadata are generated from
`Profile.xlsx` in two steps through a checked-in JSON subset
(`crates/zerofit-codegen/src/extract.rs`, `generate.rs`;
`crates/zerofit-profile/codegen/profile-subset.json`).

**Why.** The profile is data, not code. Generation removes transcription
errors, makes SDK upgrades a command, keeps all Garmin-derived content in
one crate for licensing, and lets CI verify the generated code without the
SDK.

**Tradeoffs.** Generated code is large (about 300 KB of source for 8
messages) and must be regenerated, not edited. The subset JSON is itself an
SDK derivative (flagged for license review). Only main fields are generated;
subfields (one field meaning different things depending on another field,
like `event.data`) and component expansion are not yet, and the raw value
remains available.

### 5.4 Error design

**Decision.** `Error { offset: u64, kind: ErrorKind }`
(`crates/zerofit/src/error.rs`) is `Copy + Eq` plain data; `ErrorKind` is a
`#[non_exhaustive]` enum whose variants carry the useful numbers
(`UnexpectedEof { needed }`, `FileCrcMismatch { stored, computed }`,
`UndefinedLocalMessage(local)`). The offset is absolute in the stream, and
always the start of the structure that failed. I/O errors are a separate
std-only `ReadError` (`read.rs`). The decoder is fused after an error.

**Why.** `Copy` errors work in `no_std`, can be compared in tests
(`assert_eq!(err, Error::new(...))`), and cost nothing to return. Byte
offsets make a corrupt file debuggable with a hex editor. `needed` is what
makes streaming possible: "need 3 more bytes" is both an error for the
slice decoder and a "read more" signal for the stream (§5.5).

**Tradeoffs.** No error chaining or messages with context strings; no
recovery or resynchronisation after an error (the decoder stops, by design,
because FIT has no sync markers to recover with). `#[non_exhaustive]` means
users need a wildcard arm.

### 5.5 Streaming

**Decision.** `ReadDecoder<R: Read>` reuses the slice decoder's record parser
(`parse_record`) instead of duplicating it, with a bounded buffer and owned
definitions (`crates/zerofit/src/read.rs`).

**Why.** One parser means one set of bugs and identical errors and offsets
in both modes, which is tested exhaustively: every chunk size and every
truncation point of a sample file
(`matches_slice_decoder_for_every_chunk_size`,
`truncation_errors_match_slice_decoder`), random inputs (proptest
`arbitrary_input_matches_slice_decoder`), all fixtures
(`streaming_decoder_matches_slice_decoder_on_fixtures`) and a dedicated
differential fuzz target (`fuzz/fuzz_targets/differential.rs`).

**Tradeoffs.** It is a lending iterator (`while let Some(r) =
d.next_record()`), not an `Iterator`, because records borrow the internal
buffer. The file CRC can only be verified at the end, so a corrupt stream
yields records before the error (documented on `ReadDecoder`). It needs
`std`. Getting it past the borrow checker required splitting the definition
and data paths (see §8).

## 6. Benchmarks: what they show and why we are faster

Setup (`crates/zerofit-bench/`): each fixture is loaded into memory and
decoded by five workloads (`crates/zerofit-bench/src/lib.rs`):

| Workload | What it does |
|---|---|
| `zerofit_records` | Iterate every record (CRC checked); touch the global number |
| `zerofit_all_fields` | ... and decode every field value |
| `zerofit_profile_fields` | ... and look up each field in the profile and scale it |
| `zerofit_stream` | `zerofit_records` through `ReadDecoder` |
| `fitparser` | `fitparser::from_bytes`: owned, named, scaled records |

Results on an Intel Core Ultra 7 165U laptop, rustc 1.98.1 (full table in
the README), in MiB/s:

| Fixture | records | all fields | profile-scaled | streaming | fitparser |
|---|---:|---:|---:|---:|---:|
| icu_intervals (334 KiB) | 816 | 86.6 | 68.4 | 503 | 2.42 |
| wahoo_elemnt (933 KiB) | 856 | 121 | 77.0 | 515 | 2.72 |

Allocations per decode: zerofit 0 (streaming: 5-8, its buffer and
definition slots); fitparser 3,176 to 1,581,064 (up to 310 MB allocated
for a 933 KiB file).

**What they show.** The apples-to-apples comparison is
`zerofit_profile_fields` vs `fitparser` (both produce scaled values for
every field): 20-28x. If you only need some messages (a session summary,
laps), lazy decoding means you pay only for record iteration: 160-340x.

**Why we are faster.**

1. **No allocation.** fitparser builds a `Vec` of records, each with a
   `Vec` of fields with `String` names and boxed values: 50-60
   allocations and 7-12 KB per message on the fixtures. zerofit's records are
   views into the input: a message is a slice plus a copied 64-byte
   definition.
2. **Lazy decoding.** Nothing is decoded until asked for, so iterating
   records costs a header byte, a table lookup and a slice split (~45 ns
   per message including the CRC).
3. **No per-field metadata work.** fitparser resolves names, units and
   types for every field of every message; zerofit's profile accessor is a
   direct field-number lookup with a compile-time scale.
4. **Optimized hot paths**, found by profiling (rough timing harness, then
   criterion before/after):
   - *CRC slicing-by-8* (`crates/zerofit/src/crc.rs`, `Crc16::update`). The
     file CRC was ~80% of record iteration; the byte-wise table CRC is a
     serial dependency chain. Eight independent lookups per 8 bytes:
     record iteration 2.2-3.4x faster.
   - *Inlining the per-field path* (`Value::decode` and friends in
     `value.rs`/`message.rs`). Field decoding cost ~32 ns per field because
     non-generic library functions are not inlined across crates without
     LTO; marking them `#[inline]` let the compiler fuse decode, sentinel
     check and conversion: ~10 ns per field, 2-4x throughput.
   - *Generated field index* (`MessageInfo::field`): a linear search over
     up to ~150 fields became one table load; up to 1.26x for
     profile-scaled decoding.

**Caveats to volunteer.** Laptop, hybrid cores: ±10% run to run (one run
was discarded because the machine was clearly disturbed). fitparser does more
work by design (owned, named output that you can keep after dropping the
buffer); the comparison says what each costs, not that fitparser is badly
written.

## 7. How correctness is verified

| What | Where | Catches |
|---|---|---|
| FitCSVTool ground truth | `xtask/src/expected.rs`, `crates/zerofit/tests/fixtures.rs`, `crates/zerofit-profile/tests/fixtures.rs` | Wrong counts, values, scaling, CRC handling on real files |
| Independent header/CRC check | `independent` module in `xtask/src/expected.rs` | A shared bug in our CRC/header code and the expectations |
| Generated error cases (96) | `crates/zerofit/tests/error_fixtures.rs` | Wrong error kind or offset; panics on corruption |
| Unit + property tests | `#[cfg(test)]` in every `src/*.rs` | CRC vs SDK reference, every base type both byte orders, encoder round trips, stream vs slice |
| Fuzzing | `fuzz/fuzz_targets/*.rs`, replayed by `crates/zerofit/tests/fuzz_corpus.rs` | Panics, hangs, divergence between decoders, encode/decode asymmetry |
| Lints | `[workspace.lints]` in `Cargo.toml` | `unwrap`, indexing, overflow, missing docs, `unsafe` |
| CI | `.github/workflows/ci.yml` | All of the above on Linux/macOS/Windows, `no_std`, MSRV, codegen drift |

Key points to make:

- **Expected values never come from our own decoder.** `cargo xtask expected`
  runs Garmin's FitCSVTool and maps its CSV back to numbers with the profile
  subset. Raw spot-check values are reconstructed by un-scaling FitCSVTool's
  output (`unscale`: `raw = (value + offset) * scale`, which must round to an
  integer, otherwise the tool refuses).
- **The harness tests itself.** `harness_self_test` in
  `crates/zerofit/tests/fixtures.rs` feeds deliberately wrong expectations
  and asserts every mismatch is reported, so a broken harness cannot pass.
- **Error offsets are computed independently.** `walk` in
  `error_fixtures.rs` re-parses the file layout without zerofit, and the
  expected `needed` for a truncated definition comes from its stage
  boundaries (fixed part, fields, developer count, developer fields).

## 8. The hardest bugs, and what found them

**What fuzzing found.** Honestly: no bug in the library. Three targets ran
for an hour each with 4 workers (160.8M executions of `decode`, 10.7M of
`differential`, 8.4M of `roundtrip`; `fuzz/README.md`), with no crash, hang,
divergence between the two decoders, or encode/decode asymmetry. The one
failure was in a *harness*: `roundtrip` assumed any re-encoded output
decodes, but nothing decodes from an empty input, so the encoder correctly
produced zero bytes, which is (correctly) a decode error. That input is kept
in the regression replay (`crates/zerofit/tests/fuzz_corpus.rs`).

Why so little: panic-freedom is enforced at compile time (clippy denies
indexing, `unwrap` and unchecked arithmetic in library code), every length
comes from `split_at_checked`/`get`, and the parser was already covered by
proptests on random input. The value of the fuzzing is the *differential*
and *round-trip* oracles, which check semantics, not just "no crash", and
the minimized corpus that now runs in `cargo test`.

Bugs and surprises found by the other checks:

1. **Manufacturer range markers counted as messages** (found by FitCSVTool
   ground truth). The Wahoo fixture's count of "named" messages disagreed by
   4,600. The profile's `mesg_num` type lists `mfg_range_min = 0xFF00`, so
   our harness, and `zerofit_profile::message_name`, treated Wahoo's
   private message 0xFF00 as a named message, while FitCSVTool calls it
   unknown. It is a range marker, not a message. Fixed in `message_name`
   (`crates/zerofit-profile/src/lib.rs`) and the harness.
2. **Coordinates hiding in undocumented fields** (found by the privacy
   check on the anonymizer). After moving all GPS fields the profile knows
   about, a scan of the FitCSVTool dump still found the original location:
   Wahoo writes semicircle coordinates into private message 65280 and
   decimal-degree coordinates **as strings** in message 65284, plus a
   developer field named `serial_number`. The anonymizer
   (`crates/zerofit/examples/fit-anonymize.rs`) gained a "safety net" that
   clears any non-coordinate field whose value is within a degree of the
   track (as semicircles, floats or numeric strings), and developer fields
   whose description mentions "serial".
3. **The invalid sentinel is not "all 0xFF"** (found in review of the
   anonymizer). Writing `0xFF..` to clear a signed field produces `-1`, a
   valid value; the sentinel for sint32 is `0x7FFFFFFF`, which is
   byte-order dependent. `invalid_bytes` builds it per type and per byte
   order, with a unit test.
4. **The borrow checker vs the streaming decoder.** Returning a record
   borrowed from the buffer on one path and refilling the buffer on another
   is NLL "problem case #3": rejected even though it is safe. The fix was
   structural: header and file-end records are `Record<'static>` (they own
   their data), `ensure_record` makes sure the record is buffered *before*
   the borrowing parse happens, and definition records are stored first and
   then re-borrowed from the owned slot (`record` and `store_definition` in
   `read.rs`).
5. **CRC timing differs between decoders** (found by the error-fixture
   test). Flipping the first data byte gives `FileCrcMismatch` from the
   slice decoder (it checks first) but a structural error from the stream
   (it reaches the broken record before the CRC). This is inherent, so it
   is now documented on `ReadDecoder` and the test distinguishes value
   corruption (both report the CRC) from structural corruption.

## 9. 15 likely interview questions

**1. Walk me through what happens when I call `Decoder::new(bytes)` and
iterate.**
Construction stores the slice and a 16-slot definition table on the stack;
no parsing. The first `next()` parses the header, verifies the header CRC and
the whole file's CRC, and yields `Record::Header`. Each later `next()` calls
`parse_record`: one header byte decides definition vs data; definitions are
parsed (borrowing their field list) and stored in their slot; data messages
look up their slot, slice out `message_size` bytes and resolve the
timestamp. Fields are only decoded when you call `field.value()`. At
`data_end` it yields `FileEnd` and, if bytes remain, starts another file.
(`crates/zerofit/src/decoder.rs`: `next_record`, `file_header`, `record`,
`parse_record`, `data_message`.)

**2. How do you guarantee it never panics?**
Three layers. Lints: `unwrap_used`, `expect_used`, `panic`,
`indexing_slicing`, `arithmetic_side_effects` and `unreachable` are `deny`
in `[workspace.lints.clippy]`, so code that *could* panic does not compile;
we use `get`, `split_at_checked`, `first_chunk` and `saturating_*`. Tests:
96 corruption cases, truncation at every byte of sample files, proptests on
random input. Fuzzing: three targets, about 179 million executions with no crash in
the library. What lints cannot rule out: stack overflow (no recursion) and
infinite loops (every step advances `pos`, asserted by the fuzz target).

**3. Why is `Definition` `Copy`, and why does every `DataMessage` carry a
copy of it?**
Because its field list is a borrowed slice of the input rather than a
`Vec`, it is just two slices and a few integers. Carrying a copy means a
`DataMessage<'a>` borrows only the input, not the decoder, so `Decoder` can
be a normal `Iterator` and you can keep messages after advancing. The cost
is a ~64-byte copy per message, which is cheaper than any allocation and
measured in the benchmarks.

**4. How does the streaming decoder share code with the slice decoder?**
Through `parse_record`, which takes the remaining bytes, their absolute
offset, bytes left in the data section, the last timestamp and a lookup
closure, and returns the record plus the new state. On short input it
returns `UnexpectedEof { needed }`. The slice decoder treats that as an
error; `ReadDecoder` first makes sure enough bytes are buffered
(`record_len` + `fill_to`) so `parse_record` only sees short input at true
end of file, where its error is exactly the slice decoder's.

**5. Why isn't `ReadDecoder` an `Iterator`?**
Its records borrow its internal buffer, which the next call overwrites. An
`Iterator::Item` cannot borrow from the iterator itself (that would need a
lending iterator / GATs in the trait). So it exposes `next_record(&mut self)
-> Option<Result<Record<'_>, ReadError>>` and you use `while let`. The
alternative, copying each record into an owned type, would reintroduce the
allocations we avoid.

**6. Why check the file CRC up front in the slice decoder?**
When the whole file is in memory, checking first means a corrupt file
produces no data at all, so callers cannot act on garbage. It costs one
pass over the bytes (about 80% before the slicing-by-8 optimization, far less after of the records-only time) and can
be disabled with `DecodeOptions`. The stream cannot do this and checks at
the end instead.

**7. How do compressed timestamps work and what can go wrong?**
The header carries 5 bits of the timestamp. You take the last full
timestamp, replace its low 5 bits, and add 32 if that went backwards
(`expand_timestamp`). Pitfalls: a compressed message before any full
timestamp has no timestamp (we return `None` rather than guessing); a field
253 holding the invalid sentinel must not become the reference (tested in
`invalid_timestamp_field_is_ignored`); and gaps over 31 seconds are
ambiguous by design, which is why writers must send a full timestamp at
least that often.

**8. How do you know your values are right?**
Garmin's FitCSVTool is the reference. `cargo xtask expected` runs it on
every fixture and records counts, raw values of the first and last instance
of each profile message, and every scaled session value. The xtask is not
allowed to depend on our crates. The raw layer matches all counts and spot
checks, and the profile layer matches all 106 session values within half a
scale step. Header and CRC expectations come from a separate minimal
implementation.

**9. What does the code generator do, and why check generated code in?**
It extracts the 8 messages, their types and the message-number table from
`Profile.xlsx` into JSON, then generates typed views, enums and metadata.
Checked in because the SDK cannot be redistributed or required at build
time, and because reviewers and docs.rs need real source. CI regenerates
from the JSON and fails on any diff, so it cannot drift.

**10. How does scaling work without `libm`?**
FIT defines value = raw / scale - offset, which needs only division and
subtraction, both in `core`. We never round in the library; the test
harness compares with a tolerance of half a raw step (`0.5 / scale`). The
generated accessor returns `f64` for scaled fields and the native integer
for unscaled ones.

**11. A device writes `heart_rate` as uint16 instead of uint8. What
happens?**
The raw layer decodes it as the base type the *definition* says (uint16),
because that is what the bytes are. The profile accessor
`read::int::<u8>` converts through `i64` with `TryFrom`, so a valid value
that fits is returned and one that does not is `None` rather than wrapped.
Arrays where the profile expects a scalar use the first element, like the
SDK.

**12. How are developer fields handled?**
The raw layer exposes them as bytes with their developer index, field
number and byte order (`DeveloperField`). Interpreting them needs the
file's `field_description` messages, so `zerofit-profile`'s `DeveloperData`
(behind `alloc`) collects those as you decode (`observe`) and resolves each
message's developer fields to name, units, typed value and scaled value
(`resolve`). On the Wahoo fixture all 49,288 developer fields resolve.

**13. What did fuzzing find, and how do you know the fuzzing was
meaningful?**
Nothing in the library: about 179 million executions across three targets
in an hour each, no crash. The one failure was a harness bug (empty input
in the round-trip target). I'd argue it was still meaningful because:
coverage plateaued early (the decoder is small: 472 edges for `decode`),
the seeds included real-file prefixes and every protocol feature, and two of
the targets check *semantics*: the streaming decoder must equal the slice
decoder record for record at random chunk sizes, and decode → encode →
decode must be the identity. Those would catch logic bugs that do not crash.
The minimized corpus (418 inputs) is replayed by `cargo test`, so the
coverage is not lost.

**14. What would you change for a v2?**
Generate subfields and component expansion (`event.data` meaning depends
on `event.event`; `compressed_speed_distance` packs two values). Add a
lending-iterator-friendly typed stream (`ReadDecoder` + `Message`). Offer
an `async` reader. Consider making `DataMessage` reference the decoder's
definition slot instead of copying it, if profiling ever shows the copy
matters. Generate the full profile behind feature flags so binary size
stays proportional to use.

**15. Why is this faster than `fitparser`?**
Mostly by not doing work. fitparser materializes every record as owned
`Vec`s of named, boxed field values: up to 1.6 million allocations for a
1 MB file. zerofit returns views into the input and decodes a field only
when asked, so fully decoding and scaling every field is 20-28x faster and
just walking the records 160-340x faster, with zero allocations. On top of
that, three measured optimizations: slicing-by-8 CRC (2.2-3.4x on record
iteration), `#[inline]` on the per-field path so it fuses across crates
(2-4x on field decoding), and a generated field-number index for profile
metadata. The tradeoff is that our messages borrow the buffer; if you need
to keep data after dropping the buffer, you copy what you need.

---

## 10. zerofit-analytics: training metrics on top of the decoder

`crates/zerofit-analytics` turns decoded rides into the numbers training
platforms show. Same rules as the decoder: `#![no_std]` (+ `alloc`),
panic-free by lint, every public item documented with its formula and
source. The pipeline is FIT bytes → `fit::read_fit` (raw records, timer
events) → `resample::resample` (1 Hz `ActivityStream`) → metric functions
on slices, or `analyze_stream`/`analyze_fit` for all of them at once
(`src/summary.rs`).

### 10.1 Why the resampler matters more than any formula

Every metric is a sum or rolling window over "one sample per second", but
devices don't record one sample per second. Smart recording skips seconds,
auto-pause stops recording, sensors drop out, and some files repeat a
timestamp. Each choice changes every number downstream:

- A 30 s stop counted as zeros lowers average power and NP. Removing it
  doesn't.
- Interpolating across a 10-minute café stop invents 10 minutes of riding.
- A duplicated timestamp counted twice adds a second of work that never
  happened.

`src/resample.rs` makes each decision explicit, tests it and counts it in
a `ResampleReport`:

- **Duplicates** merge field by field, with the later record winning.
- **Backwards timestamps** are dropped.
- **Gaps over 30 s** are pauses and are removed, so the stream is
  recording time.
- **Gaps of 1–30 s** are linearly interpolated in every channel.
- **Field dropouts up to 8 s** are repaired. Longer ones become 0 W for
  power and missing for HR.
- **Power over 2500 W** is treated as a spike.

The 30 s and 8 s numbers are intervals.icu's, from its developer's forum
posts. The payoff: on `icu_laps`, our stream's total work equals the
`total_work` intervals.icu wrote into its export *to the joule*. That's
the strongest evidence that both sides are working on the same series.

### 10.2 The metrics in plain English

- **Average power** = total work / recording time. Coasting zeros count;
  paused time doesn't exist in the stream.
- **Normalized power (Coggan)**: take a 30 s rolling average, raise each
  value to the 4th power, average those, take the 4th root. The 30 s
  window mimics how slowly the body responds. The 4th power reflects that
  physiological cost rises steeply with intensity, so a ride of surges
  "costs" more than its average suggests. Our implementation uses exact
  integer rolling sums (`u64`), so there's no float drift over six hours.
  It starts at the first full window. (`power::normalized_power`)
- **IF** = NP / FTP: how hard the ride was relative to threshold.
- **TSS** = hours × IF² × 100. One hour at FTP is 100 by definition;
  doubling duration doubles it, and 10 % more intensity adds 21 %.
  *Which* hours is a convention: TrainingPeaks uses file duration,
  intervals.icu uses moving time. We default to moving time and take the
  duration as a parameter. (`power::tss`, `summary::LoadDuration`)
- **VI** = NP / average: 1.0 is a steady time trial, 1.3 a criterium.
- **hrTSS**: Banister's TRIMP sums, per minute, the heart-rate-reserve
  fraction x weighted by `0.64·e^(1.92x)`, because lactate rises
  exponentially with HR. Dividing by the TRIMP of one hour at LTHR makes
  an hour at threshold score 100, comparable with TSS. (`hr::hr_tss`)
- **Efficiency factor** = NP / average HR: watts per beat. It rising over
  weeks of similar rides means aerobic fitness improving.
- **Pa:HR decoupling**: power per heartbeat in the first half of the ride
  versus the second. If HR drifts up at the same power (heat,
  dehydration, a weak aerobic base), the ratio falls and decoupling is
  positive. Under 5 % on a long steady ride is good. (`hr::decoupling`)
- **Time in zones**: seconds in each Coggan power zone (% FTP) and Friel
  HR zone (% LTHR), with upper bounds inclusive as the tables are written.
- **Mean-maximal power curve**: for every duration, the best average power
  held that long (§10.3).
- **Critical power and W'**: CP is the power you can sustain for a long
  time, and W' is a fixed tank of work above it. At P > CP you empty the
  tank at P − CP joules per second, so `P(t) = W'/t + CP`. We fit it by
  linear least squares on `work = CP·t + W'` over log-spaced 2–20 minute
  points, and report R², RMSE and standard errors. The 3-parameter Morton
  model `P = W'/(t−k) + CP` adds a finite max power. It's fitted by
  golden-section search over k with an inner linear solve. (`src/cp.rs`)
- **W' balance (Skiba, differential)**: second by second, above CP the
  tank drains by P − CP. Below CP, the *spent* part recovers exponentially
  at rate (CP − P)/W'. That's one O(n) pass with no fitted time constant;
  intervals.icu uses the same model. It can go negative, which says the
  athlete's CP/W' settings are too low; we don't clamp that away.
  (`src/wbal.rs`)
- **CTL / ATL / TSB**: fitness and fatigue as exponentially weighted
  averages of daily TSS, with 42- and 7-day time constants. Form is
  fitness minus fatigue. (`src/load.rs`)
- **eFTP**: each maximal 3–30 minute effort (d, P) places the athlete on
  their own hyperbola `W'/t + CP_d` through that point. eFTP is the
  highest such curve's value at one hour:
  `max_d (MMP(d) − W'/d) + W'/3600`. With W' = 20 kJ it gives about 96 %
  of 20-minute power, which is the familiar 95 % rule falling out of the
  model. (`load::estimate_ftp`)

### 10.3 The MMP algorithm and its complexity

With prefix sums `S`, the sum of any window is `S[i+d] − S[i]`. So for one
duration d the best window is a single pass over two offset slices, and
the whole curve is `Σ_d (n − d + 1) ≈ n²/2` operations. That's 233 million
for a 6-hour ride.

Can you do better exactly? Computing the maximum window sum for every
length at once is a (max,+) convolution, and no truly subquadratic
algorithm is known for it, so the honest optimization is the constant
factor (`src/mmp.rs`):

- prefix sums in `i32` whenever the total work fits, which it always does
  for real rides;
- the inner loop written as 16 independent lane maxima over
  `chunks_exact`, which is what the auto-vectorizer handles;
- signed rather than unsigned, because baseline x86-64 (SSE2) has a
  signed 32-bit compare but no unsigned one.

The assembly confirms `psubd`/`pcmpgtd`/blend on four lanes. The result
is 75 ms for a 6-hour curve (about 3 G windows/s) on a laptop. When only
a few durations are needed, `mmp_at` is O(n·k). Larger totals fall back
to wrapping `u32` and then `u64`.

### 10.4 Two "obvious" invariants that are false

The brief asked for property tests of "NP ≥ average power" and "the MMP
curve is non-increasing". proptest falsified both, and the docs had
claimed the second with a wrong proof:

- **MMP can increase with duration.** `[980, 0, 980]`: the best 2 s
  averages 490 W, the best 3 s 653 W. The "drop the weakest second"
  argument fails because removing a middle second doesn't leave a window.
  On the real fixtures the curve rises at thousands of durations and sits
  up to 8 % below its envelope: two hard efforts with a lull between
  them. What is true, and now tested: best *work* never decreases with
  duration; MMP(k·d) ≤ MMP(d) (split the window into k pieces); MMP(1) is
  the max and MMP(n) the average. `PowerCurve::envelope` gives the
  monotone curve when a model needs one.
- **NP can be below the average** at the edges. The first and last 29
  seconds sit in fewer than 30 windows, so one hard second followed by
  30 s of zeros gives NP 28 W against an average of 32 W. NP is always ≥
  the mean of its rolling averages (power-mean inequality), and ≥ the
  average when the ride starts and ends with ≥ 29 s of zeros. Both
  versions are tested.

### 10.5 How validation works

Validation works in four layers, none of which uses the crate to check
itself:

1. **Hand-computed unit tests.** Constant power gives NP = average. One
   hour at FTP gives TSS 100. A 60 s/60 s block test checks NP window by
   window against an explicit sum. All zeros gives NP 0 and no NaN. W'bal
   is checked against a hand-stepped sequence, CP recovery against an
   exact hyperbola, and CTL against the closed form `L·(1 − e^(−n/42))`.
2. **Brute force.** The MMP curve is compared to an O(n³) window
   enumeration.
3. **proptest invariants** (`tests/proptest.rs`):
   - the corrected MMP and NP properties above;
   - W'bal ≤ W';
   - TSS exactly linear in duration at constant power, and never lower
     when riding continues;
   - resampler accounting (every output second is a kept record or a
     filled gap; elapsed = output + paused);
   - full-pipeline consistency.
4. **intervals.icu** (`tests/fixtures.rs`):
   - values recorded by hand from the web app
     (`tests/fixtures/intervals_icu.json`, still to be filled in);
   - the session values intervals.icu writes into its own FIT exports,
     with the FTP it used. Every metric gets an explicit tolerance, and
     the test prints the error table.

### 10.6 How close we match intervals.icu, and why we differ

Against intervals.icu's export (FTP 323 W):

- **Total work**: exact on one ride, 0.007 % on the other.
- **Average power**: within 0.3 %.
- **IF**: within 0.9 %.
- **NP**: 0.05 % on `icu_intervals`, −0.8 % on `icu_laps`.
- **TSS**: −0.7 % and +1.45 %.

Each difference was investigated before anything was tuned (nothing was):

- **TSS**: NP and IF match, so the difference is all in the duration.
  intervals.icu uses moving time from the velocity stream with an
  unpublished threshold. A threshold fitted to one ride misfits the other,
  so the default stays a documented 0.5 m/s. Using recording time instead
  would be 3 % off.
- **NP on `icu_laps`**: the streams are identical (the work matches to
  the joule), so it's algorithmic. I tried several variants and none fits
  both rides: per-segment windows, partial first windows, EWMA smoothing,
  dropping zeros. The textbook definition stays; the gap is documented as
  unexplained and under 1 %.
- **Average power**: intervals.icu counts P + 2 fewer recording seconds
  for P pauses, about 0.1 %.

### 10.7 intervals.icu conventions vs the textbook

| Topic | Textbook / TrainingPeaks | intervals.icu (our default) |
|---|---|---|
| TSS duration | file duration | moving time |
| CTL/ATL daily weight | 1/τ | 1 − e^(−1/τ) (exact decay) |
| TSB on day t | yesterday's CTL − ATL | today's, after training |
| Power curve | recording time | elapsed time, pauses as 0 W |
| Short power dropouts | varies | repaired up to 8 s |
| hrTSS | TrainingPeaks: time in zones → TSS/h | HRSS: normalized TRIMP |
| eFTP | 95 % of 20 min | best ≥ 3 min effort placed on a model curve, read at 1 h |

The two weights differ by 1.2 % per day for τ = 42 but 7.3 % for τ = 7, so
platforms agree on fitness and disagree visibly on fatigue and form.

### 10.8 Performance

Measured on a Core Ultra 7 165U laptop:

| Workload | Time |
|---|---|
| Full analysis of a 4 h ride (resampling, every metric, the full curve, CP, eFTP, W'bal) | 56 ms, mostly the O(n²) curve |
| 6 h MMP curve | 75 ms |
| 24 selected durations | 0.9 ms |
| CTL/ATL over 3 years of days | 12 µs |
| `analyze_fit` on a real 1-hour file | 6 ms |

### 10.9 WebAssembly

`crates/zerofit-wasm` (originally `zerofit-analytics-wasm`) exports
`analyze(fitBytes, settingsJson) → summaryJson`. The logic is a plain
Rust function (`analyze_json`) with native tests; the `#[wasm_bindgen]`
export only converts the error type. That's also the only place `unsafe`
is allowed: the macro's FFI glue, with a documented `#[allow]`. CI builds
for `wasm32-unknown-unknown`, runs `wasm-bindgen` (pinned to the crate's
version) and a node smoke test that checks NP/TSS on a real ride, an
HR-only ride and the error paths. The `.wasm` is about 220 KB.

### 10.10 Ten more likely questions

**1. Why `no_std` for an analytics library?**
The metrics are pure arithmetic over slices, so nothing needs an OS. That
lets the same code run on a bike computer's microcontroller, in a browser
via WebAssembly, or on a server. It needs `alloc`, though: a stream is as
long as the ride, so a fixed-size buffer would cap ride length
arbitrarily. Per-metric functions borrow slices and don't allocate; only
the resampler, the curve and the load series do. `core` has no
`exp`/`sqrt`, so they come from `libm`.

**2. Why is power stored as `u16` with dropouts as 0, while HR is
`Option<u8>`?**
Every power metric needs a value every second. NP's rolling window can't
skip a second, and work is a sum. So power gets a concrete value plus a
dropout flag (`ActivityStream::power_dropout`), and metrics take `&[u16]`
directly. Missing heart rate has no honest substitute, so HR metrics skip
`None` instead of averaging in zeros.

**3. Your TSS doesn't match TrainingPeaks for the same file. Bug?**
Probably a convention. TrainingPeaks uses the file's duration;
intervals.icu, and our default, use moving time, so a café stop with the
timer running adds no load. `LoadDuration::Recording` reproduces the
TrainingPeaks convention. NP and IF should match either way; if they
don't, look at how each tool fills gaps and pauses.

**4. How do you know the resampler is right?**
Each rule is a unit test. A property test checks the accounting:

- every output second is a kept record or a filled gap;
- elapsed time = output seconds + paused seconds;
- timestamps strictly increase;
- no panics on arbitrary input.

Externally, our stream's total work equals intervals.icu's exported
`total_work` to the joule on one ride and within 0.007 % on another.

**5. Why is the 6-hour curve 75 ms and not 5 ms?**
It's 233 million window evaluations, and the exact all-durations problem
has no known subquadratic algorithm, so the only lever is the constant.
We're at about 3 G evaluations/s with SSE2. AVX2's `vpmaxsd` would roughly
double that, but a library shouldn't force `target-cpu` flags on its
users. If 75 ms matters, compute `mmp_at` for the 20 durations you plot,
or update a season curve incrementally per activity (`SeasonCurve::add`)
instead of recomputing.

**6. Why fit CP on log-spaced points instead of every second of 2–20
minutes?**
One-second steps would put 90 % of the points between 3 and 20 minutes
and weight the fit toward the long end. Log spacing (about 10 % steps)
weights each "decade" of duration equally. The model is also only valid
in roughly 2–20 minutes: shorter efforts aren't W'-limited and longer
ones hit fatigue the model ignores.

**7. W'bal went negative. Is that a bug?**
No. It means the athlete did more work above CP than the model's W'
allows, so CP or W' is set too low. Clamping at zero would hide exactly
the evidence you need to update the settings. W'bal never *exceeds* W',
because recovery approaches it asymptotically, and a property test
checks that.

**8. What does a pause do to W'bal and NP?**
NP's window runs across a removed pause: the stream is recording time,
and inventing zeros would penalise stopping. W'bal treats the pause as
rest at 0 W for its real duration, using the closed form
`e^(−CP·Δt/W')`, because the athlete was recovering, not frozen in time.
The stream keeps each sample's elapsed time for exactly this.

**9. How would you make CTL/ATL match another platform exactly?**
Three knobs, all in `LoadConfig`:

- the daily weight (`Convention`);
- whether TSB uses today's or yesterday's values;
- the seed values (intervals.icu seeds from 184 days of history, so start
  your series early or pass `initial_ctl`/`initial_atl`).

Then make sure the daily loads match: same TSS convention, and multiple
activities on one day summed (`daily_loads`).

**10. What would you add next?**
In order:

1. Fill in `intervals_icu.json` and run the comparison on more rides.
2. Interval detection.
3. Running metrics: rTSS from normalized graded pace.
4. An incremental, streaming version of the power curve for on-device
   use, where only the K standard durations are kept as O(K) rolling
   windows.
5. Fitting CP to the season envelope rather than single rides.
