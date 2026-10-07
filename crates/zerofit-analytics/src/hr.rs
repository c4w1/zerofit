//! Heart-rate metrics: average HR, hrTSS (normalized TRIMP), efficiency
//! factor and Pa:HR decoupling.
//!
//! Heart rate is `&[Option<u8>]`, one value per second; `None` (a missing
//! or dropped-out reading) is skipped, never treated as 0 bpm.

use crate::TrimpCoefficients;
use crate::num::f64_from_usize;

/// Mean heart rate, bpm, over the seconds that have a value. `None` if
/// none do.
///
/// ```
/// use zerofit_analytics::hr::average_hr;
/// assert_eq!(average_hr(&[Some(100), None, Some(120)]), Some(110.0));
/// assert_eq!(average_hr(&[None]), None);
/// ```
#[must_use]
pub fn average_hr(hr: &[Option<u8>]) -> Option<f64> {
    let (sum, n) = hr.iter().flatten().fold((0u64, 0usize), |(s, n), &h| {
        (s.saturating_add(u64::from(h)), n.saturating_add(1))
    });
    (n > 0).then(|| crate::num::f64_from_u64(sum) / f64_from_usize(n))
}

/// Heart-rate thresholds for [`hr_tss`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HrThresholds {
    /// Resting heart rate, bpm.
    pub resting: f64,
    /// Lactate threshold heart rate, bpm.
    pub threshold: f64,
    /// Maximum heart rate, bpm.
    pub max: f64,
}

/// Banister's TRIMP for one second at heart rate `hr`, in "TRIMP
/// minutes": `x · a·e^(b·x) / 60`, with `x` the heart-rate-reserve
/// fraction clamped to `[0, 1]`.
fn trimp_second(hr: f64, t: &HrThresholds, (a, b): (f64, f64)) -> f64 {
    let x = ((hr - t.resting) / (t.max - t.resting)).clamp(0.0, 1.0);
    x * a * libm::exp(b * x) / 60.0
}

/// hrTSS: heart-rate training stress, as Banister TRIMP normalised so that
/// one hour at LTHR scores 100 (the "HRSS" model in intervals.icu and
/// Golden Cheetah).
///
/// ```text
/// x(HR)  = (HR − HR_rest) / (HR_max − HR_rest)          (clamped to 0…1)
/// TRIMP  = Σ_seconds  x · a·e^(b·x) / 60                 (Banister 1991)
/// hrTSS  = TRIMP / (60 · x_LT · a·e^(b·x_LT)) · 100      x_LT = x(LTHR)
/// ```
///
/// `(a, b)` is `(0.64, 1.92)` or `(0.86, 1.67)`, see
/// [`TrimpCoefficients`]. The exponential weighting reflects blood lactate
/// rising exponentially with heart-rate reserve, so an hour at threshold
/// costs far more than two hours at a low heart rate.
///
/// Judgment calls:
///
/// - **Missing seconds are skipped**, so dropouts lower hrTSS (there is no
///   honest value to fill in).
/// - **HR above max or below rest is clamped**, so a spike can't make the
///   exponential explode.
/// - **Which seconds**: the caller decides; the activity summary passes
///   moving seconds only, like intervals.icu, which uses "the moving time
///   in each heart rate zone".
///
/// This is not TrainingPeaks' hrTSS, which maps time in HR zones to
/// TSS-per-hour values; no two platforms agree on HR-based load, and the
/// TRIMP form is the one with a published formula.
///
/// `None` unless `rest < LTHR < max`.
///
/// ```
/// use zerofit_analytics::{TrimpCoefficients, hr::{HrThresholds, hr_tss}};
/// let t = HrThresholds { resting: 50.0, threshold: 165.0, max: 190.0 };
/// // One hour at LTHR is 100 by construction.
/// let hour = vec![Some(165); 3600];
/// let score = hr_tss(&hour, &t, TrimpCoefficients::Male).unwrap();
/// assert!((score - 100.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn hr_tss(hr: &[Option<u8>], t: &HrThresholds, coefficients: TrimpCoefficients) -> Option<f64> {
    if !(t.resting < t.threshold && t.threshold < t.max) {
        return None;
    }
    let ab = coefficients.coefficients();
    let trimp: f64 = hr
        .iter()
        .flatten()
        .map(|&h| trimp_second(f64::from(h), t, ab))
        .sum();
    let hour_at_threshold = trimp_second(t.threshold, t, ab) * 3600.0;
    Some(trimp / hour_at_threshold * 100.0)
}

/// Efficiency factor (Friel; TrainingPeaks): `EF = NP / average HR`.
///
/// Watts produced per heartbeat-per-minute. Comparing EF across similar
/// aerobic rides shows aerobic fitness improving (EF rises). `None` if the
/// average HR is not positive.
///
/// ```
/// use zerofit_analytics::hr::efficiency_factor;
/// assert_eq!(efficiency_factor(210.0, 140.0), Some(1.5));
/// ```
#[must_use]
pub fn efficiency_factor(np: f64, average_hr: f64) -> Option<f64> {
    (average_hr > 0.0).then(|| np / average_hr)
}

/// Pa:HR (power-to-heart-rate) aerobic decoupling, %.
///
/// ```text
/// split the samples at the midpoint of the ride
/// rₖ = average power / average HR       for each half k
/// decoupling = (r₁ − r₂) / r₁ · 100
/// ```
///
/// If heart rate drifts up while power stays the same (cardiac drift from
/// dehydration, heat or insufficient aerobic fitness), r₂ falls and
/// decoupling is positive. Friel's guideline: under 5 % on a long steady
/// ride means the aerobic base is good (Friel, *The Cyclist's Training
/// Bible*; Allen & Coggan).
///
/// Judgment calls:
///
/// - **Average power, not NP.** intervals.icu's formula (as described by
///   users on its forum) uses average power per half; TrainingPeaks'
///   Pw:Hr uses the ratio of the halves' EF. They agree on steady rides,
///   the only rides where decoupling means anything.
/// - **Sign.** Positive = heart rate drifted up relative to power, as in
///   Friel and intervals.icu's display.
/// - **Halves by sample count**, i.e. by recording time; seconds without
///   heart rate are skipped within each half.
///
/// `None` if either half has no heart rate or zero power.
///
/// ```
/// use zerofit_analytics::hr::decoupling;
/// let power = [200u16; 200];
/// let mut hr = vec![Some(140u8); 100];
/// hr.extend(vec![Some(147u8); 100]); // 5 % drift
/// let d = decoupling(&power, &hr).unwrap();
/// assert!((d - 4.7619).abs() < 1e-3); // (1/140 − 1/147)/(1/140)
/// ```
#[must_use]
pub fn decoupling(power: &[u16], hr: &[Option<u8>]) -> Option<f64> {
    let n = power.len().min(hr.len());
    let mid = n / 2;
    let (p1, p2) = power.get(..n)?.split_at_checked(mid)?;
    let (h1, h2) = hr.get(..n)?.split_at_checked(mid)?;
    let r1 = half_ratio(p1, h1)?;
    let r2 = half_ratio(p2, h2)?;
    Some((r1 - r2) / r1 * 100.0)
}

/// Average power over seconds with HR, divided by their average HR.
fn half_ratio(power: &[u16], hr: &[Option<u8>]) -> Option<f64> {
    let (sum_p, sum_h, n) = power
        .iter()
        .zip(hr)
        .filter_map(|(&p, h)| h.map(|h| (p, h)))
        .fold((0u64, 0u64, 0usize), |(sp, sh, n), (p, h)| {
            (
                sp.saturating_add(u64::from(p)),
                sh.saturating_add(u64::from(h)),
                n.saturating_add(1),
            )
        });
    if n == 0 || sum_h == 0 || sum_p == 0 {
        return None;
    }
    // avg P / avg HR = (ΣP/n) / (ΣHR/n) = ΣP / ΣHR. Zero-power seconds
    // count, as in average power.
    Some(crate::num::f64_from_u64(sum_p) / crate::num::f64_from_u64(sum_h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const T: HrThresholds = HrThresholds {
        resting: 50.0,
        threshold: 160.0,
        max: 190.0,
    };

    #[test]
    fn hr_tss_scales_with_time_and_is_100_per_hour_at_threshold() {
        let one = hr_tss(&vec![Some(160); 3600], &T, TrimpCoefficients::Male).unwrap();
        let two = hr_tss(&vec![Some(160); 7200], &T, TrimpCoefficients::Female).unwrap();
        assert!((one - 100.0).abs() < 1e-9);
        assert!((two - 200.0).abs() < 1e-9);
    }

    #[test]
    fn hr_tss_hand_computed() {
        // 30 min at 120 bpm: x = 70/140 = 0.5; x_LT = 110/140.
        let x: f64 = 0.5;
        let xl: f64 = 110.0 / 140.0;
        let expected = 1800.0 * x * (1.92 * x).exp() / (3600.0 * xl * (1.92 * xl).exp()) * 100.0;
        let got = hr_tss(&vec![Some(120); 1800], &T, TrimpCoefficients::Male).unwrap();
        assert!((got - expected).abs() < 1e-9, "{got} vs {expected}");
    }

    #[test]
    fn hr_tss_clamps_and_skips() {
        let below = hr_tss(&[Some(30), None], &T, TrimpCoefficients::Male).unwrap();
        assert_eq!(below, 0.0);
        let over = hr_tss(&[Some(250)], &T, TrimpCoefficients::Male).unwrap();
        let at_max = hr_tss(&[Some(190)], &T, TrimpCoefficients::Male).unwrap();
        assert!((over - at_max).abs() < 1e-12);
        let bad = HrThresholds {
            threshold: 200.0,
            ..T
        };
        assert_eq!(hr_tss(&[Some(150)], &bad, TrimpCoefficients::Male), None);
    }

    #[test]
    fn decoupling_steady_is_zero() {
        let d = decoupling(&[200; 100], &[Some(140); 100]).unwrap();
        assert!(d.abs() < 1e-12);
    }

    #[test]
    fn decoupling_skips_missing_hr_and_needs_both_halves() {
        let mut hr = vec![Some(140u8); 50];
        hr.extend(vec![None; 50]);
        assert_eq!(decoupling(&[200; 100], &hr), None);
        assert_eq!(decoupling(&[], &[]), None);
        assert_eq!(decoupling(&[0; 10], &[Some(100); 10]), None);
    }

    #[test]
    fn ef_and_average() {
        assert_eq!(efficiency_factor(200.0, 0.0), None);
        assert_eq!(average_hr(&[]), None);
    }
}
