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
//!    [`AthleteSettings`]; [`analyze_stream`] (or [`analyze_records`],
//!    or `analyze_fit` from FIT bytes) runs all of them at once.
//!
//! ```
//! use zerofit_analytics::{ActivityStream, AnalysisConfig, AthleteSettings, analyze_stream};
//!
//! // 10 minutes easy, 20 minutes at 300 W, 10 minutes easy.
//! let mut power = vec![150u16; 600];
//! power.extend([300u16; 1200]);
//! power.extend([150u16; 600]);
//! let stream = ActivityStream::from_power(0, &power);
//!
//! let athlete = AthleteSettings { ftp: Some(280.0), ..AthleteSettings::default() };
//! let summary = analyze_stream(&stream, &athlete, &AnalysisConfig::default()).summary;
//! assert_eq!(summary.average_power, Some(225.0));
//! let np = summary.normalized_power.unwrap();
//! assert!(np > 250.0 && np < 260.0);
//! println!("NP {np:.0} W, IF {:.2}, TSS {:.0}", summary.intensity_factor.unwrap(), summary.tss.unwrap());
//! ```
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
pub mod load;
pub mod mmp;
mod num;
pub mod power;
pub mod resample;
pub mod stream;
pub mod summary;
pub mod wbal;

pub use athlete::{AthleteSettings, DEFAULT_W_PRIME, TrimpCoefficients, Zones};
pub use stream::{ActivityStream, DEFAULT_MOVING_SPEED, Sample, SampleState};
#[cfg(feature = "fit")]
pub use summary::analyze_fit;
pub use summary::{ActivitySummary, Analysis, AnalysisConfig, analyze_records, analyze_stream};
