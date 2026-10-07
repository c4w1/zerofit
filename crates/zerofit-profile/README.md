# zerofit-profile

Typed FIT profile layer for [`zerofit`](../zerofit): named, scaled accessors
and Rust enums for the messages that make up an activity file (`file_id`,
`record`, `lap`, `session`, `event`, `device_info`, `activity`, `hrv`).

`no_std`, no allocation, zero-copy: every typed view wraps a
`zerofit::DataMessage` that borrows the input.

```rust
use zerofit::{Decoder, Record as RawRecord};
use zerofit_profile::Message;

# fn main() -> Result<(), zerofit::Error> {
# let bytes: &[u8] = &[];
for item in Decoder::new(bytes) {
    if let RawRecord::Data(msg) = item? {
        if let Message::Record(r) = Message::new(msg) {
            println!("{:?} bpm, {:?} m/s", r.heart_rate(), r.speed());
        }
    }
}
# Ok(())
# }
```

## How it is built

`src/generated/` is produced by `zerofit-codegen` and checked in. The input
is `codegen/profile-subset.json`, a trimmed extract of the FIT SDK's
`Profile.xlsx` (only the messages above, the types they use, and the
message-number table). CI regenerates the code from that file and fails if
it differs:

```sh
cargo xtask codegen --check                        # verify
cargo xtask codegen                                # regenerate from the subset
FIT_PROFILE_XLSX=/path/to/FitSDKRelease_21.171.00/Profile.xlsx \
    cargo xtask extract                            # refresh the subset from a new SDK
```

## FIT SDK license notice

The FIT protocol and profile are defined by Garmin International, Inc. The
contents of `codegen/profile-subset.json` and `src/generated/` (message,
field and type names and numbers, scales, offsets, units and descriptions)
are derived from `Profile.xlsx` in the FIT SDK, which is distributed under
the [FIT Protocol License](https://developer.garmin.com/fit/download/).
Those files are subject to that license in addition to this crate's
MIT/Apache-2.0 license, which covers the hand-written code. Review the FIT
SDK license before redistributing them. The FIT SDK itself, including
`Profile.xlsx`, is not included in this repository.
