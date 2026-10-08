//! Power metrics: average power, work, normalized power, intensity factor,
//! training stress score and variability index.
//!
//! All functions take power as `&[u16]`, one value per second of recording
//! time (see [`resample`](crate::resample) for how pauses and dropouts get
//! there), and don't allocate.

use crate::num::{f64_from_u64, f64_from_usize};

/// Window of the rolling average in normalized power, s.
pub const NP_WINDOW: usize = 30;

/// Mean power, W, over every sample, zeros included.
///
/// `P̄ = Σ Pᵢ / n`. Coasting zeros are part of the ride and count; paused
/// time is not in the stream and doesn't. intervals.icu's average is the
/// same quantity, "Joules / recording time". `None` for an empty slice.
///
/// ```
/// use zerofit_analytics::power::average_power;
/// assert_eq!(average_power(&[100, 200, 0]), Some(100.0));
/// assert_eq!(average_power(&[]), None);
/// ```
#[must_use]
pub fn average_power(power: &[u16]) -> Option<f64> {
    if power.is_empty() {
        return None;
    }
    Some(f64_from_u64_sum(power) / f64_from_usize(power.len()))
}

/// Mechanical work, kJ: `Σ Pᵢ · 1 s / 1000`.
///
/// For cycling, kJ of work ≈ kcal of food energy burned, because gross
/// efficiency (~20–25 %) roughly cancels the 4.184 kJ/kcal conversion.
///
/// ```
/// use zerofit_analytics::power::work_kj;
/// assert_eq!(work_kj(&[250; 3600]), 900.0);
/// ```
#[must_use]
pub fn work_kj(power: &[u16]) -> f64 {
    f64_from_u64_sum(power) / 1000.0
}

/// Maximum power, W. `None` for an empty slice.
///
/// ```
/// use zerofit_analytics::power::max_power;
/// assert_eq!(max_power(&[100, 900, 300]), Some(900));
/// ```
#[must_use]
pub fn max_power(power: &[u16]) -> Option<u16> {
    power.iter().copied().max()
}

/// Normalized power (Coggan), W.
///
/// ```text
/// rᵢ = (P[i-29] + … + P[i]) / 30         for i = 29 … n-1
/// NP = ( Σ rᵢ⁴ / (n - 29) ) ^ (1/4)
/// ```
///
/// A 30 s rolling average approximates the time course of the body's
/// physiological response; raising it to the 4th power weights hard
/// efforts the way blood lactate and glycogen use rise with intensity.
/// NP is "the power you could have held steadily for the same
/// physiological cost" (Allen & Coggan, *Training and Racing with a Power
/// Meter*, 2nd ed., ch. 7).
///
/// Judgment calls:
///
/// - **The first 29 seconds produce no rolling value.** The average starts
///   at the first full 30 s window (TrainingPeaks' and WKO's convention).
///   Some tools average partial windows over the first 29 s instead; on
///   real rides the two differ by well under 0.1 %.
/// - **Zeros count.** Coasting is part of the ride and lowers the rolling
///   average, as in the original definition.
/// - **Windows span pauses.** The stream is recording time, so the window
///   continues across a removed pause instead of filling it with zeros.
/// - **Fewer than 30 samples: `None`.** There is no full window to
///   average, and falling back to average power would silently change the
///   metric's meaning.
///
/// Rolling sums are exact integer arithmetic (`u64`), so there is no
/// floating-point drift over long rides; O(n) time, no allocation.
///
/// ```
/// use zerofit_analytics::power::normalized_power;
/// // Constant power: NP equals it.
/// let np = normalized_power(&[200; 600]).unwrap();
/// assert!((np - 200.0).abs() < 1e-9);
/// assert_eq!(normalized_power(&[200; 29]), None);
/// ```
#[must_use]
pub fn normalized_power(power: &[u16]) -> Option<f64> {
    if power.len() < NP_WINDOW {
        return None;
    }
    let (head, _) = power.split_at_checked(NP_WINDOW)?;
    let mut window_sum: u64 = head.iter().map(|&p| u64::from(p)).sum();
    let mut sum_fourth = fourth(window_sum);
    // Slide: add P[i], drop P[i-30]. `power[30..]` zipped with `power[..]`
    // pairs each new sample with the one leaving the window.
    let entering = power.get(NP_WINDOW..).unwrap_or_default();
    for (&add, &drop) in entering.iter().zip(power) {
        // Never underflows: `drop` is part of `window_sum`.
        window_sum = window_sum
            .wrapping_add(u64::from(add))
            .wrapping_sub(u64::from(drop));
        sum_fourth += fourth(window_sum);
    }
    let windows = f64_from_usize(power.len().saturating_sub(NP_WINDOW - 1));
    Some(libm::sqrt(libm::sqrt(sum_fourth / windows)))
}

/// `(window_sum / 30)^4`.
fn fourth(window_sum: u64) -> f64 {
    let mean = f64_from_u64(window_sum) / 30.0;
    let sq = mean * mean;
    sq * sq
}

/// Intensity factor (Coggan): `IF = NP / FTP`.
///
/// 1.0 is a ride at threshold for its whole duration; typical values are
/// 0.65–0.75 for endurance rides and 0.95–1.05 for a 40 km time trial.
/// `None` unless `ftp > 0`.
///
/// ```
/// use zerofit_analytics::power::intensity_factor;
/// assert_eq!(intensity_factor(200.0, 250.0), Some(0.8));
/// assert_eq!(intensity_factor(200.0, 0.0), None);
/// ```
#[must_use]
pub fn intensity_factor(np: f64, ftp: f64) -> Option<f64> {
    (ftp > 0.0).then(|| np / ftp)
}

/// Training stress score (Coggan).
///
/// ```text
/// TSS = (t · NP · IF) / (FTP · 3600) · 100  =  t/3600 · IF² · 100
/// ```
///
/// with `t` in seconds. One hour at FTP is exactly 100; the score grows
/// linearly with duration and quadratically with intensity (Allen &
/// Coggan; TrainingPeaks, "Estimating Training Stress Score").
///
/// Judgment call: **which `t`.** TrainingPeaks uses the duration of the
/// file. intervals.icu uses *moving time*, so a café stop with the timer
/// running doesn't add load ([`analyze_stream`](crate::analyze_stream) passes
/// [`ActivityStream::moving_time`](crate::ActivityStream::moving_time)).
/// This function takes `t` explicitly so either convention can be used.
///
/// `None` unless `ftp > 0`.
///
/// ```
/// use zerofit_analytics::power::tss;
/// // One hour at FTP is 100 by definition.
/// assert_eq!(tss(3600.0, 250.0, 250.0), Some(100.0));
/// // Two hours at 75 % of FTP: 2 · 0.75² · 100 = 112.5.
/// assert!((tss(7200.0, 187.5, 250.0).unwrap() - 112.5).abs() < 1e-9);
/// ```
#[must_use]
pub fn tss(duration_s: f64, np: f64, ftp: f64) -> Option<f64> {
    let intensity = intensity_factor(np, ftp)?;
    Some(duration_s * np * intensity / (ftp * 3600.0) * 100.0)
}

/// Variability index: `VI = NP / average power`.
///
/// How steady the ride was: ~1.00–1.05 for a time trial or a trainer
/// session, 1.1–1.3 for a hilly group ride or a criterium (Allen &
/// Coggan). `None` if the average is 0.
///
/// ```
/// use zerofit_analytics::power::variability_index;
/// assert_eq!(variability_index(220.0, 200.0), Some(1.1));
/// assert_eq!(variability_index(0.0, 0.0), None);
/// ```
#[must_use]
pub fn variability_index(np: f64, average: f64) -> Option<f64> {
    (average > 0.0).then(|| np / average)
}

fn f64_from_u64_sum(power: &[u16]) -> f64 {
    f64_from_u64(power.iter().map(|&p| u64::from(p)).sum())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * b.abs().max(1.0)
    }

    #[test]
    fn constant_power_np_equals_average() {
        for w in [1u16, 150, 250, 1000] {
            let p = [w; 3600];
            assert!(close(normalized_power(&p).unwrap(), f64::from(w)));
            assert!(close(average_power(&p).unwrap(), f64::from(w)));
        }
    }

    #[test]
    fn zeros() {
        let p = [0u16; 120];
        assert_eq!(normalized_power(&p), Some(0.0));
        assert_eq!(average_power(&p), Some(0.0));
        assert_eq!(work_kj(&p), 0.0);
        assert_eq!(variability_index(0.0, 0.0), None);
    }

    #[test]
    fn exactly_one_window() {
        let mut p = [0u16; 30];
        p[0] = 300;
        // One window, mean 10 W.
        assert!(close(normalized_power(&p).unwrap(), 10.0));
    }

    /// 60 s at 100 W then 60 s at 300 W, computed window by window:
    /// 31 windows at 100 W, 29 transition windows at 100 + 200·k/30,
    /// 31 windows at 300 W.
    #[test]
    fn two_blocks_hand_computed() {
        let mut p = Vec::new();
        p.extend([100u16; 60]);
        p.extend([300u16; 60]);
        let mut sum = 31.0 * 100f64.powi(4) + 31.0 * 300f64.powi(4);
        for k in 1..=29 {
            sum += (100.0 + 200.0 * f64::from(k) / 30.0).powi(4);
        }
        let expected = (sum / 91.0).powf(0.25);
        assert!(close(normalized_power(&p).unwrap(), expected));
        // ≈ 244.0 W against an average of 200 W.
        assert!((expected - 244.04).abs() < 0.01, "{expected}");
        assert_eq!(average_power(&p), Some(200.0));
    }

    /// Classic interval session: 10 × (1 min at 400 W, 1 min at 100 W).
    /// The 30 s rolling average never reaches either level, so NP sits
    /// well above the 250 W average.
    #[test]
    fn interval_blocks() {
        let mut p = Vec::new();
        for _ in 0..10 {
            p.extend([400u16; 60]);
            p.extend([100u16; 60]);
        }
        let np = normalized_power(&p).unwrap();
        assert_eq!(average_power(&p), Some(250.0));
        assert!((np - 315.58).abs() < 0.01, "{np}");
    }

    #[test]
    fn tss_and_if() {
        assert_eq!(tss(3600.0, 250.0, 250.0), Some(100.0));
        assert!(close(tss(1800.0, 250.0, 250.0).unwrap(), 50.0));
        assert_eq!(tss(3600.0, 250.0, 0.0), None);
        assert_eq!(intensity_factor(250.0, -1.0), None);
    }

    #[test]
    fn work() {
        assert_eq!(work_kj(&[1000; 1]), 1.0);
        assert_eq!(max_power(&[]), None);
    }
}
