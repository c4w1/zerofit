//! Training analytics for 1 Hz activity streams.
//!
//! `zerofit-analytics` turns a decoded activity into the numbers training
//! platforms show: average and normalized power, intensity factor, TSS,
//! variability index, work, hrTSS, efficiency factor, Pa:HR decoupling,
//! time in zones, the mean-maximal power curve, critical power and W'
//! fits, W' balance, and across activities CTL/ATL/TSB, the season power
//! curve and estimated FTP.
//!
//! Every metric's documentation gives its formula, its source, and the
//! judgment calls an implementation has to make (how zeros, pauses and
//! the first 30 seconds are treated, for example). Where intervals.icu
//! does something different from the textbook, the default follows
//! intervals.icu and the difference is documented.
//!
//! # Pipeline
//!
//! 1. Raw records (from FIT with the `fit` feature, or from anywhere else)
//!    become an [`ActivityStream`] with one sample per second of recording
//!    time. See [`resample`] for exactly how gaps, pauses, dropouts and
//!    duplicate timestamps are treated, because every metric depends on it.
//! 2. Metric functions take the stream (or plain slices) plus
//!    [`AthleteSettings`].
//!
//! # `no_std`
//!
//! The crate is `#![no_std]` and needs `alloc`: a stream is as long as the
//! activity, so fixed-size buffers would cap ride length arbitrarily. The
//! per-metric functions borrow slices and don't allocate; only the
//! resampler, the power curve and the load series do. Float math that
//! `core` lacks (`exp`, `sqrt`) comes from `libm`.
//!
//! # Features
//!
//! - `fit` (default): read activities from FIT bytes with `zerofit` and
//!   `zerofit-profile`. Without it the crate has no FIT dependency.
//! - `serde`: `Serialize` for the result types.
#![no_std]
// Exact float comparisons are intended in tests of hand-computed values.
#![cfg_attr(test, allow(clippy::float_cmp))]

extern crate alloc;

pub mod athlete;
pub mod cp;
#[cfg(feature = "fit")]
pub mod fit;
pub mod hr;
pub mod mmp;
mod num;
pub mod power;
pub mod resample;
pub mod stream;

pub use athlete::{AthleteSettings, DEFAULT_W_PRIME, TrimpCoefficients, Zones};
pub use stream::{ActivityStream, DEFAULT_MOVING_SPEED, Sample, SampleState};
