//! Across activities: fitness, fatigue and form (CTL/ATL/TSB), the season
//! power curve, and estimated FTP.
//!
//! # The performance manager model
//!
//! Banister's impulse-response model (Banister et al. 1975) treats each
//! day's training load as an impulse that raises both fitness and fatigue,
//! each decaying exponentially with its own time constant. Coggan's
//! Performance Manager simplifies it to two exponentially weighted moving
//! averages of daily TSS:
//!
//! - **CTL** (chronic training load, "fitness"): time constant 42 days.
//! - **ATL** (acute training load, "fatigue"): time constant 7 days.
//! - **TSB** (training stress balance, "form") = CTL − ATL.
//!
//! The decay per day for time constant `τ` can be written two ways, and
//! platforms differ:
//!
//! | | intervals.icu ([`Convention::IntervalsIcu`]) | TrainingPeaks ([`Convention::TrainingPeaks`]) |
//! |---|---|---|
//! | Update | `CTLₜ = CTLₜ₋₁·e^(−1/τ) + loadₜ·(1 − e^(−1/τ))` | `CTLₜ = CTLₜ₋₁ + (loadₜ − CTLₜ₋₁)/τ` |
//! | TSB on day t | `CTLₜ − ATLₜ` (after today's training) | `CTLₜ₋₁ − ATLₜ₋₁` (form going into today) |
//!
//! The exponential form is the exact discretisation of a continuous decay
//! with time constant `τ`; `1/τ` is its first-order approximation. They
//! differ by 1.2 % in the daily weight for τ = 42 but 7.3 % for τ = 7, so
//! ATL and TSB differ visibly between platforms. Sources: users on the
//! intervals.icu forum reproduced its wellness values with the
//! exponential form, and its developer states that form is shown "after
//! that day's training"; TrainingPeaks documents the `1/τ` form and
//! yesterday's values for TSB. Days without training are days with load
//! 0, and several activities on one day add up ([`daily_loads`]).

use alloc::vec::Vec;

use crate::cp::{FitRange, fit_curve_2p};
use crate::mmp::PowerCurve;
use crate::num::f64_from_usize;

/// How the daily update and TSB are computed. See the [module
/// docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Convention {
    /// Exponential decay `e^(−1/τ)`, TSB from today's values (default).
    #[default]
    IntervalsIcu,
    /// Linear `1/τ` weight, TSB from yesterday's values.
    TrainingPeaks,
}

/// Settings for [`training_load`].
///
/// ```
/// use zerofit_analytics::load::LoadConfig;
/// let c = LoadConfig::default();
/// assert_eq!((c.ctl_days, c.atl_days, c.initial_ctl), (42.0, 7.0, 0.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadConfig {
    /// CTL time constant, days. Default 42.
    pub ctl_days: f64,
    /// ATL time constant, days. Default 7.
    pub atl_days: f64,
    /// CTL before the first day. Default 0 (no history).
    pub initial_ctl: f64,
    /// ATL before the first day. Default 0.
    pub initial_atl: f64,
    /// Update and TSB convention.
    pub convention: Convention,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            ctl_days: 42.0,
            atl_days: 7.0,
            initial_ctl: 0.0,
            initial_atl: 0.0,
            convention: Convention::IntervalsIcu,
        }
    }
}

/// One day of the performance manager series.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DailyLoad {
    /// The day's total training load (TSS).
    pub load: f64,
    /// Fitness at the end of the day.
    pub ctl: f64,
    /// Fatigue at the end of the day.
    pub atl: f64,
    /// Form: see [`Convention`].
    pub tsb: f64,
}

/// CTL/ATL/TSB for consecutive days of training load.
///
/// `loads[i]` is the total load of day `i`. O(n), one allocation.
///
/// ```
/// use zerofit_analytics::load::{LoadConfig, training_load};
/// // 100 TSS every day: CTL approaches 100 as 1 − e^(−t/42).
/// let days = training_load(&[100.0; 42], &LoadConfig::default());
/// let expected = 100.0 * (1.0 - (-1.0f64).exp());
/// assert!((days[41].ctl - expected).abs() < 1e-9);
/// ```
#[must_use]
pub fn training_load(loads: &[f64], config: &LoadConfig) -> Vec<DailyLoad> {
    let weight = |tau: f64| match config.convention {
        Convention::IntervalsIcu => 1.0 - libm::exp(-1.0 / tau),
        Convention::TrainingPeaks => 1.0 / tau,
    };
    let (ctl_w, atl_w) = (weight(config.ctl_days), weight(config.atl_days));
    let (mut ctl, mut atl) = (config.initial_ctl, config.initial_atl);
    loads
        .iter()
        .map(|&load| {
            let yesterday_tsb = ctl - atl;
            ctl += (load - ctl) * ctl_w;
            atl += (load - atl) * atl_w;
            let tsb = match config.convention {
                Convention::IntervalsIcu => ctl - atl,
                Convention::TrainingPeaks => yesterday_tsb,
            };
            DailyLoad {
                load,
                ctl,
                atl,
                tsb,
            }
        })
        .collect()
}

/// Sums `(day, load)` pairs into one load per day, `0..=last day`; days
/// without activities are 0.
///
/// ```
/// use zerofit_analytics::load::daily_loads;
/// assert_eq!(daily_loads(&[(0, 50.0), (2, 30.0), (2, 20.0)]), vec![50.0, 0.0, 50.0]);
/// ```
#[must_use]
pub fn daily_loads(activities: &[(usize, f64)]) -> Vec<f64> {
    let days = activities
        .iter()
        .map(|&(d, _)| d.saturating_add(1))
        .max()
        .unwrap_or(0);
    let mut out = alloc::vec![0.0; days];
    for &(day, load) in activities {
        if let Some(slot) = out.get_mut(day) {
            *slot += load;
        }
    }
    out
}

/// The best power for every duration across many activities, remembering
/// which activity set each point.
///
/// ```
/// use zerofit_analytics::{load::SeasonCurve, mmp::PowerCurve};
/// let mut season = SeasonCurve::default();
/// season.add(7, &PowerCurve::new(&[900, 100]));
/// season.add(8, &PowerCurve::new(&[300, 300, 300]));
/// assert_eq!(season.curve().watts(1), Some(900.0));
/// assert_eq!(season.source(1), Some(7));
/// assert_eq!(season.source(2), Some(7)); // 900 + 100 J beats 300 + 300 J
/// assert_eq!(season.source(3), Some(8));
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SeasonCurve {
    curve: PowerCurve,
    source: Vec<u64>,
}

impl SeasonCurve {
    /// Merges an activity's curve in; `activity` is any caller-chosen id.
    pub fn add(&mut self, activity: u64, curve: &PowerCurve) {
        for d in self.curve.merge_max(curve) {
            let Some(i) = d.checked_sub(1) else { continue };
            if self.source.len() <= i {
                self.source.resize(i.saturating_add(1), activity);
            }
            if let Some(slot) = self.source.get_mut(i) {
                *slot = activity;
            }
        }
    }

    /// The combined curve.
    #[must_use]
    pub const fn curve(&self) -> &PowerCurve {
        &self.curve
    }

    /// The activity that holds the best power for `duration_s`.
    #[must_use]
    pub fn source(&self, duration_s: usize) -> Option<u64> {
        self.source.get(duration_s.checked_sub(1)?).copied()
    }
}

/// Estimated FTP and how it was derived.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct EftpEstimate {
    /// Estimated FTP, W: the model's power at 1 hour.
    pub eftp: f64,
    /// The effort that set it: duration, s.
    pub duration_s: usize,
    /// The effort that set it: power, W.
    pub watts: f64,
    /// W' of the curve shape used, J.
    pub w_prime: f64,
}

/// Estimated FTP from a power curve (an activity's or a season's).
///
/// ```text
/// eFTP = max over d in [min_s, max_s] of ( MMP(d) − W'/d ) + W'/3600
/// ```
///
/// Each maximal effort of duration `d` places the athlete on a
/// power-duration hyperbola `P(t) = W'/t + CP_d` through that point; the
/// highest such curve wins, and eFTP is its value at one hour.
///
/// This mirrors how intervals.icu describes its eFTP (its developer, on
/// the forum: a single max effort of at least 3 minutes "place\[s\] you on
/// one of many pre-defined power curves … the 1h point on that curve is
/// your eFTP", "a generalisation of the 95 % of 20m power rule"). The
/// difference: intervals.icu's curve family is empirical and
/// unpublished; here the curve shape is the athlete's own W', fitted from
/// the same curve's 2–20 minute points ([`fit_curve_2p`]), or
/// `fallback_w_prime` if the fit fails or its W' is outside
/// [`PLAUSIBLE_W_PRIME`]. With W' = 20 kJ and CP = 250 W the
/// formula gives 95.8 % of 20-minute power, in line with the rule of
/// thumb.
///
/// Durations default to intervals.icu's 3 minutes up to "about 30
/// minutes" ([`EFTP_RANGE`]). Returns `None` if the curve is shorter than
/// `range.min_s`.
///
/// ```
/// use zerofit_analytics::{load::{EFTP_RANGE, estimate_ftp}, mmp::PowerCurve};
/// let mut ride = vec![150u16; 600];
/// ride.extend(vec![300u16; 1200]); // a 20-minute effort at 300 W
/// ride.extend(vec![150u16; 600]);
/// let est = estimate_ftp(&PowerCurve::new(&ride), EFTP_RANGE, 20_000.0).unwrap();
/// // The flat 2-20 min curve gives no usable W', so the 20 kJ fallback
/// // applies: 300 − 20000/1200 + 20000/3600 ≈ 288.9 W.
/// assert_eq!(est.duration_s, 1200);
/// assert!((est.eftp - 288.89).abs() < 0.01);
/// ```
#[must_use]
pub fn estimate_ftp(
    curve: &PowerCurve,
    range: FitRange,
    fallback_w_prime: f64,
) -> Option<EftpEstimate> {
    let w_prime = plausible_w_prime(
        fit_curve_2p(curve, FitRange::TWO_PARAMETER).map(|fit| fit.w_prime),
        fallback_w_prime,
    );
    let max = range.max_s.min(curve.max_duration());
    best_on_hyperbola(
        (range.min_s.max(1)..=max).filter_map(|d| Some((d, curve.watts(d)?))),
        w_prime,
    )
}

/// [`estimate_ftp`] from sampled `(duration_s, watts)` points of a power
/// curve instead of the full curve, e.g. a season curve stored at
/// log-spaced durations. W' is fitted from the points between 2 and
/// 20 minutes; eFTP is the best over the points inside `range`, so its
/// accuracy depends on how densely the points cover 3–30 minutes.
///
/// ```
/// use zerofit_analytics::load::{EFTP_RANGE, estimate_ftp_from_points};
/// // Points exactly on W'/t + CP with CP 260 W, W' 18 kJ.
/// let points: Vec<(usize, f64)> = [60, 120, 180, 300, 600, 1200, 1800, 3600]
///     .iter().map(|&d| (d, 18_000.0 / d as f64 + 260.0)).collect();
/// let est = estimate_ftp_from_points(&points, EFTP_RANGE, 20_000.0).unwrap();
/// assert!((est.eftp - (260.0 + 18_000.0 / 3600.0)).abs() < 1e-6);
/// ```
#[must_use]
pub fn estimate_ftp_from_points(
    points: &[(usize, f64)],
    range: FitRange,
    fallback_w_prime: f64,
) -> Option<EftpEstimate> {
    let fit_points: Vec<(f64, f64)> = points
        .iter()
        .filter(|(d, _)| {
            (FitRange::TWO_PARAMETER.min_s..=FitRange::TWO_PARAMETER.max_s).contains(d)
        })
        .map(|&(d, w)| (f64_from_usize(d), w))
        .collect();
    let w_prime = plausible_w_prime(
        crate::cp::fit_2p(&fit_points).map(|fit| fit.w_prime),
        fallback_w_prime,
    );
    best_on_hyperbola(
        points
            .iter()
            .copied()
            .filter(|(d, _)| (range.min_s.max(1)..=range.max_s).contains(d)),
        w_prime,
    )
}

fn plausible_w_prime(fitted: Option<f64>, fallback: f64) -> f64 {
    fitted
        .filter(|w| PLAUSIBLE_W_PRIME.contains(w))
        .unwrap_or(fallback)
}

/// The highest `MMP(d) − W'/d + W'/3600` over `points`.
fn best_on_hyperbola(
    points: impl Iterator<Item = (usize, f64)>,
    w_prime: f64,
) -> Option<EftpEstimate> {
    points
        .map(|(d, watts)| EftpEstimate {
            eftp: watts - w_prime / f64_from_usize(d) + w_prime / 3600.0,
            duration_s: d,
            watts,
            w_prime,
        })
        .max_by(|a, b| a.eftp.total_cmp(&b.eftp))
}

/// The range of fitted W' that [`estimate_ftp`] trusts, J.
///
/// Judgment call: trained cyclists' W' is typically 15–25 kJ (Skiba
/// 2012), and values far outside 5–50 kJ come from curves that aren't
/// maximal across 2–20 minutes (a single long effort makes the curve flat
/// there, and the fitted W' collapses towards 0).
pub const PLAUSIBLE_W_PRIME: core::ops::RangeInclusive<f64> = 5_000.0..=50_000.0;

/// Durations eFTP is estimated from: 3 to 30 minutes.
pub const EFTP_RANGE: FitRange = FitRange {
    min_s: 180,
    max_s: 1800,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_form_ctl_after_constant_load() {
        // CTL_n = L·(1 − e^(−n/42)) with the exponential update.
        let days = training_load(&[80.0; 100], &LoadConfig::default());
        for (n, d) in (1..).zip(&days) {
            let expected = 80.0 * (1.0 - libm::exp(-f64::from(n) / 42.0));
            assert!((d.ctl - expected).abs() < 1e-9);
            let atl = 80.0 * (1.0 - libm::exp(-f64::from(n) / 7.0));
            assert!((d.atl - atl).abs() < 1e-9);
            assert!((d.tsb - (d.ctl - d.atl)).abs() < 1e-12);
        }
    }

    #[test]
    fn trainingpeaks_convention() {
        let cfg = LoadConfig {
            convention: Convention::TrainingPeaks,
            ..LoadConfig::default()
        };
        let days = training_load(&[42.0, 0.0], &cfg);
        assert!((days[0].ctl - 1.0).abs() < 1e-12); // 0 + (42 − 0)/42
        assert!((days[0].atl - 6.0).abs() < 1e-12); // 0 + 42/7
        assert_eq!(days[0].tsb, 0.0); // yesterday's: 0 − 0
        assert!((days[1].tsb - (1.0 - 6.0)).abs() < 1e-12);
    }

    #[test]
    fn rest_decays_and_seeds_apply() {
        let cfg = LoadConfig {
            initial_ctl: 60.0,
            initial_atl: 60.0,
            ..LoadConfig::default()
        };
        let days = training_load(&[0.0; 7], &cfg);
        let ctl = 60.0 * libm::exp(-7.0 / 42.0);
        let atl = 60.0 * libm::exp(-1.0);
        assert!((days[6].ctl - ctl).abs() < 1e-9);
        assert!((days[6].atl - atl).abs() < 1e-9);
        assert!(days[6].tsb > 0.0); // rested: fresh form
    }

    #[test]
    fn empty() {
        assert!(training_load(&[], &LoadConfig::default()).is_empty());
        assert!(daily_loads(&[]).is_empty());
        assert!(estimate_ftp(&PowerCurve::new(&[300; 100]), EFTP_RANGE, 20_000.0).is_none());
    }

    #[test]
    fn eftp_from_exact_hyperbola_is_one_hour_power() {
        // A season curve that is exactly W'/t + CP: best work for d seconds
        // is W' + CP·d.
        let (cp, w) = (260.0, 18_000.0);
        let sums = (1..=3600u64).map(|d| 18_000 + 260 * d).collect();
        let curve = PowerCurve::from_best_sums(sums);
        let est = estimate_ftp(&curve, EFTP_RANGE, 1.0).unwrap();
        assert!((est.w_prime - w).abs() < 1e-6, "{est:?}");
        assert!((est.eftp - (cp + w / 3600.0)).abs() < 1e-6, "{est:?}");
    }

    #[test]
    fn season_tracks_sources() {
        let mut s = SeasonCurve::default();
        s.add(1, &PowerCurve::new(&[100, 100, 100]));
        s.add(2, &PowerCurve::new(&[500]));
        assert_eq!(s.source(1), Some(2));
        assert_eq!(s.source(3), Some(1));
        assert_eq!(s.source(4), None);
        assert_eq!(s.curve().max_duration(), 3);
    }
}
