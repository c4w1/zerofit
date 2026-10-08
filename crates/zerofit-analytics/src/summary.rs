//! One call that computes every per-activity metric.
//!
//! [`analyze_stream`] runs the whole pipeline on an [`ActivityStream`];
//! [`analyze_records`] resamples raw records first, and [`analyze_fit`]
//! (feature `fit`) starts from FIT bytes. The result is an [`Analysis`]:
//! a compact, serializable [`ActivitySummary`] plus the full power curve
//! and W' balance stream for callers that want to plot them.

use alloc::vec::Vec;

use crate::cp::{CpFit, FitRange, fit_curve_2p};
use crate::hr::{self, HrThresholds};
use crate::load::{EFTP_RANGE, EftpEstimate, estimate_ftp};
use crate::mmp::{PowerCurve, STANDARD_DURATIONS};
use crate::power;
use crate::resample::{RawRecord, ResampleConfig, ResampleReport, TimerEvent, resample};
use crate::stream::DEFAULT_MOVING_SPEED;
use crate::wbal::{min_balance, w_prime_balance_stream};
use crate::{ActivityStream, AthleteSettings};

/// Which duration TSS (and hrTSS) is computed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum LoadDuration {
    /// Moving time (intervals.icu's convention; default).
    #[default]
    Moving,
    /// Recording time (TrainingPeaks' convention: the file's duration).
    Recording,
}

/// Settings for the analysis pipeline.
///
/// ```
/// use zerofit_analytics::summary::AnalysisConfig;
/// let c = AnalysisConfig::default();
/// assert!(c.curve_over_elapsed_time);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalysisConfig {
    /// Resampler settings (used by [`analyze_records`] / [`analyze_fit`]).
    pub resample: ResampleConfig,
    /// Speed above which a second counts as moving, m/s.
    pub moving_speed: f64,
    /// Duration used for TSS and hrTSS.
    pub load_duration: LoadDuration,
    /// Build the power curve over elapsed time, with paused seconds as 0 W
    /// (intervals.icu's power curve; default), rather than over recording
    /// time only.
    pub curve_over_elapsed_time: bool,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            resample: ResampleConfig::default(),
            moving_speed: DEFAULT_MOVING_SPEED,
            load_duration: LoadDuration::Moving,
            curve_over_elapsed_time: true,
        }
    }
}

/// One point of the power curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CurvePoint {
    /// Duration, s.
    pub duration_s: usize,
    /// Best average power for that duration, W.
    pub watts: f64,
}

/// Every per-activity metric. Fields are `None` when their inputs are
/// missing (no power meter, no FTP set, no heart rate…).
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ActivitySummary {
    /// Recording time (samples in the stream), s.
    pub recording_time_s: usize,
    /// Elapsed time including pauses, s.
    pub elapsed_time_s: u32,
    /// Moving time, s ([`ActivityStream::moving_time`]).
    pub moving_time_s: usize,
    /// Whether the activity has power data.
    pub has_power: bool,
    /// Average power, W ([`power::average_power`]).
    pub average_power: Option<f64>,
    /// Maximum power, W.
    pub max_power: Option<u16>,
    /// Normalized power, W ([`power::normalized_power`]).
    pub normalized_power: Option<f64>,
    /// Intensity factor ([`power::intensity_factor`]).
    pub intensity_factor: Option<f64>,
    /// Training stress score ([`power::tss`]).
    pub tss: Option<f64>,
    /// Variability index ([`power::variability_index`]).
    pub variability_index: Option<f64>,
    /// Work, kJ ([`power::work_kj`]).
    pub work_kj: Option<f64>,
    /// Average power per kg of body weight, W/kg.
    pub average_watts_per_kg: Option<f64>,
    /// Average heart rate, bpm ([`hr::average_hr`]).
    pub average_hr: Option<f64>,
    /// Maximum heart rate, bpm.
    pub max_hr: Option<u8>,
    /// hrTSS ([`hr::hr_tss`]).
    pub hr_tss: Option<f64>,
    /// Efficiency factor, NP / average HR ([`hr::efficiency_factor`]).
    pub efficiency_factor: Option<f64>,
    /// Pa:HR decoupling, % ([`hr::decoupling`]).
    pub decoupling_pct: Option<f64>,
    /// Seconds in each power zone (needs FTP).
    pub power_zone_seconds: Option<Vec<u32>>,
    /// Seconds in each heart-rate zone (needs LTHR).
    pub hr_zone_seconds: Option<Vec<u32>>,
    /// The power curve at [`STANDARD_DURATIONS`] (those the ride covers).
    pub power_curve: Vec<CurvePoint>,
    /// 2-parameter CP fit of this ride's curve. Single rides rarely
    /// contain maximal efforts at every duration: check `r_squared`.
    pub cp_fit: Option<CpFit>,
    /// Estimated FTP from this ride's curve ([`estimate_ftp`]).
    pub eftp: Option<EftpEstimate>,
    /// Lowest W' balance, J ([`crate::wbal`]); uses `cp` (else FTP) and
    /// `w_prime` (else the default) from the athlete settings.
    pub min_w_prime_balance: Option<f64>,
    /// What the resampler did (only from [`analyze_records`] /
    /// [`analyze_fit`]).
    pub resample_report: Option<ResampleReport>,
}

/// The result of an analysis: the summary plus the full-resolution series.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Analysis {
    /// Every metric.
    pub summary: ActivitySummary,
    /// The full power curve (every duration).
    pub power_curve: PowerCurve,
    /// W' balance per sample, J (empty without power or CP/FTP).
    pub w_prime_balance: Vec<f64>,
}

/// Computes every metric for `stream`.
///
/// ```
/// use zerofit_analytics::{ActivityStream, AthleteSettings, summary::{AnalysisConfig, analyze_stream}};
/// let stream = ActivityStream::from_power(0, &[250; 3600]);
/// let athlete = AthleteSettings { ftp: Some(250.0), ..AthleteSettings::default() };
/// let a = analyze_stream(&stream, &athlete, &AnalysisConfig::default());
/// let s = &a.summary;
/// assert_eq!(s.average_power, Some(250.0));
/// assert!((s.tss.unwrap() - 100.0).abs() < 1e-9); // one hour at FTP
/// assert_eq!(s.power_zone_seconds.as_ref().unwrap()[3], 3600); // all Z4
/// assert_eq!(a.power_curve.watts(3600), Some(250.0));
/// ```
#[must_use]
pub fn analyze_stream(
    stream: &ActivityStream,
    athlete: &AthleteSettings,
    config: &AnalysisConfig,
) -> Analysis {
    let power = stream.power();
    let has_power = stream.has_power();
    let moving_time = stream.moving_time(config.moving_speed);
    let load_seconds = match config.load_duration {
        LoadDuration::Moving => moving_time,
        LoadDuration::Recording => stream.recording_time(),
    };
    let ftp = athlete.ftp.filter(|f| *f > 0.0);

    let mut s = ActivitySummary {
        recording_time_s: stream.recording_time(),
        elapsed_time_s: stream.elapsed_time(),
        moving_time_s: moving_time,
        has_power,
        ..ActivitySummary::default()
    };

    let mut power_curve = PowerCurve::default();
    let mut w_prime_balance = Vec::new();
    if has_power {
        s.average_power = power::average_power(power);
        s.max_power = power::max_power(power);
        s.normalized_power = power::normalized_power(power);
        s.work_kj = Some(power::work_kj(power));
        s.variability_index = s
            .normalized_power
            .zip(s.average_power)
            .and_then(|(np, avg)| power::variability_index(np, avg));
        s.average_watts_per_kg = s
            .average_power
            .zip(athlete.weight_kg.filter(|w| *w > 0.0))
            .map(|(p, kg)| p / kg);
        if let Some(ftp) = ftp {
            s.intensity_factor = s
                .normalized_power
                .and_then(|np| power::intensity_factor(np, ftp));
            s.tss = s
                .normalized_power
                .and_then(|np| power::tss(crate::num::f64_from_usize(load_seconds), np, ftp));
            s.power_zone_seconds = Some(
                athlete
                    .power_zones
                    .time_in(power.iter().map(|&p| f64::from(p)), ftp),
            );
        }

        power_curve = if config.curve_over_elapsed_time {
            PowerCurve::new(&stream.power_elapsed())
        } else {
            PowerCurve::new(power)
        };
        s.power_curve = STANDARD_DURATIONS
            .iter()
            .filter_map(|&d| {
                Some(CurvePoint {
                    duration_s: d,
                    watts: power_curve.watts(d)?,
                })
            })
            .collect();
        s.cp_fit = fit_curve_2p(&power_curve, FitRange::TWO_PARAMETER);
        s.eftp = estimate_ftp(&power_curve, EFTP_RANGE, athlete.w_prime_or_default());
        if let Some(cp) = athlete.cp_or_ftp() {
            w_prime_balance = w_prime_balance_stream(stream, cp, athlete.w_prime_or_default());
            s.min_w_prime_balance = min_balance(&w_prime_balance).map(|(_, b)| b);
        }
    }

    hr_metrics(stream, athlete, config, &mut s);

    Analysis {
        summary: s,
        power_curve,
        w_prime_balance,
    }
}

/// The heart-rate part of [`analyze_stream`].
fn hr_metrics(
    stream: &ActivityStream,
    athlete: &AthleteSettings,
    config: &AnalysisConfig,
    s: &mut ActivitySummary,
) {
    let heart_rate = stream.heart_rate();
    s.average_hr = hr::average_hr(heart_rate);
    s.max_hr = heart_rate.iter().flatten().copied().max();
    if s.average_hr.is_some() {
        if s.has_power {
            s.efficiency_factor = s
                .normalized_power
                .zip(s.average_hr)
                .and_then(|(np, avg)| hr::efficiency_factor(np, avg));
            s.decoupling_pct = hr::decoupling(stream.power(), heart_rate);
        }
        if let Some(lthr) = athlete.lthr {
            s.hr_zone_seconds = Some(athlete.hr_zones.time_in(
                heart_rate.iter().flatten().map(|&h| f64::from(h)),
                f64::from(lthr),
            ));
            if let (Some(rest), Some(max)) = (athlete.resting_hr, athlete.max_hr) {
                let thresholds = HrThresholds {
                    resting: f64::from(rest),
                    threshold: f64::from(lthr),
                    max: f64::from(max),
                };
                let load_hr: Vec<Option<u8>> = match config.load_duration {
                    LoadDuration::Moving => heart_rate
                        .iter()
                        .zip(stream.speed())
                        .map(|(&h, v)| h.filter(|_| v.is_none_or(|v| v > config.moving_speed)))
                        .collect(),
                    LoadDuration::Recording => heart_rate.to_vec(),
                };
                s.hr_tss = hr::hr_tss(&load_hr, &thresholds, athlete.trimp);
            }
        }
    }
}

/// Resamples `records` (see [`resample`]) and analyzes the result.
///
/// ```
/// use zerofit_analytics::{AthleteSettings, resample::RawRecord, summary::{AnalysisConfig, analyze_records}};
/// let records: Vec<RawRecord> = (0..120)
///     .step_by(2) // smart recording: a record every 2 s
///     .map(|t| RawRecord { power: Some(200), ..RawRecord::at(t) })
///     .collect();
/// let a = analyze_records(&records, &[], &AthleteSettings::default(), &AnalysisConfig::default());
/// assert_eq!(a.summary.recording_time_s, 119);
/// assert_eq!(a.summary.resample_report.unwrap().gap_seconds_filled, 59);
/// ```
#[must_use]
pub fn analyze_records(
    records: &[RawRecord],
    timer: &[TimerEvent],
    athlete: &AthleteSettings,
    config: &AnalysisConfig,
) -> Analysis {
    let (stream, report) = resample(records, timer, &config.resample);
    let mut analysis = analyze_stream(&stream, athlete, config);
    analysis.summary.resample_report = Some(report);
    analysis
}

/// Decodes a FIT file and analyzes it (feature `fit`).
///
/// # Errors
///
/// The first decoding error.
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// use zerofit_analytics::{AthleteSettings, summary::{AnalysisConfig, analyze_fit}};
/// let a = analyze_fit(bytes, &AthleteSettings::default(), &AnalysisConfig::default())?;
/// assert_eq!(a.summary.average_hr, Some(142.0));
/// assert!(!a.summary.has_power);
/// # Ok::<(), zerofit::Error>(())
/// ```
#[cfg(feature = "fit")]
pub fn analyze_fit(
    bytes: &[u8],
    athlete: &AthleteSettings,
    config: &AnalysisConfig,
) -> Result<Analysis, zerofit::Error> {
    let activity = crate::fit::read_fit(bytes)?;
    Ok(analyze_records(
        &activity.records,
        &activity.timer_events,
        athlete,
        config,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Sample;

    fn athlete() -> AthleteSettings {
        AthleteSettings {
            ftp: Some(250.0),
            weight_kg: Some(70.0),
            lthr: Some(160),
            max_hr: Some(190),
            resting_hr: Some(50),
            ..AthleteSettings::default()
        }
    }

    #[test]
    fn empty_stream_has_no_metrics() {
        let a = analyze_stream(
            &ActivityStream::new(0),
            &athlete(),
            &AnalysisConfig::default(),
        );
        assert_eq!(a.summary.recording_time_s, 0);
        assert_eq!(a.summary.average_power, None);
        assert_eq!(a.summary.average_hr, None);
        assert!(a.power_curve.is_empty());
    }

    #[test]
    fn hr_only_ride() {
        let mut s = ActivityStream::new(0);
        for t in 0..3600 {
            let mut sample = Sample::power(t, 0);
            sample.power = None;
            sample.heart_rate = Some(160);
            s.push(sample).unwrap();
        }
        let a = analyze_stream(&s, &athlete(), &AnalysisConfig::default());
        assert!(!a.summary.has_power);
        assert_eq!(a.summary.tss, None);
        assert!((a.summary.hr_tss.unwrap() - 100.0).abs() < 1e-9);
        assert_eq!(
            a.summary
                .hr_zone_seconds
                .as_ref()
                .unwrap()
                .iter()
                .sum::<u32>(),
            3600
        );
        assert_eq!(a.summary.efficiency_factor, None);
    }

    #[test]
    fn stopped_seconds_reduce_tss_only_with_moving_duration() {
        let mut s = ActivityStream::new(0);
        for t in 0..3600 {
            let speed = if t < 600 { 0.0 } else { 9.0 };
            s.push(Sample {
                speed: Some(speed),
                ..Sample::power(t, 250)
            })
            .unwrap();
        }
        let moving = analyze_stream(&s, &athlete(), &AnalysisConfig::default());
        assert_eq!(moving.summary.moving_time_s, 3000);
        let recording = analyze_stream(
            &s,
            &athlete(),
            &AnalysisConfig {
                load_duration: LoadDuration::Recording,
                ..AnalysisConfig::default()
            },
        );
        let (m, r) = (moving.summary.tss.unwrap(), recording.summary.tss.unwrap());
        assert!((r - 100.0).abs() < 1e-9 && (m - 100.0 * 3000.0 / 3600.0).abs() < 1e-9);
    }

    #[test]
    fn curve_over_elapsed_time_includes_pauses() {
        let mut s = ActivityStream::new(0);
        s.push(Sample::power(0, 300)).unwrap();
        s.push(Sample::power(10, 300)).unwrap();
        let elapsed = analyze_stream(&s, &athlete(), &AnalysisConfig::default());
        assert_eq!(elapsed.power_curve.max_duration(), 11);
        let recording = analyze_stream(
            &s,
            &athlete(),
            &AnalysisConfig {
                curve_over_elapsed_time: false,
                ..AnalysisConfig::default()
            },
        );
        assert_eq!(recording.power_curve.watts(2), Some(300.0));
    }
}
