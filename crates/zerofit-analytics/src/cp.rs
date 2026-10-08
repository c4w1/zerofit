//! Critical power models fitted to a power-duration curve.
//!
//! The critical power concept (Monod & Scherrer 1965; Moritani et al.
//! 1981; Jones et al. 2010) splits the ability to sustain power into a
//! rate that can be held for a long time, **CP** (W), and a finite
//! reserve of work above it, **W'** (J). Riding at `P > CP` empties W' at
//! `P − CP` joules per second, so exhaustion comes after
//! `t = W' / (P − CP)`, i.e.
//!
//! ```text
//! P(t) = W'/t + CP                       2-parameter model
//! P(t) = W'/(t − k) + CP,  k < 0         3-parameter model (Morton 1996)
//! ```
//!
//! The 3-parameter model adds `k = −W'/(Pmax − CP)`, so the curve
//! reaches a finite `Pmax` at `t = 0` instead of shooting to infinity,
//! which lets it use sprint durations too.
//!
//! Every fit reports its quality in the *power* domain: R² and RMSE of
//! predicted against actual MMP. The work-time regression's own R² is
//! nearly always above 0.99 and says little.
//!
//! Judgment calls:
//!
//! - **Which durations.** The 2-parameter model only holds between about
//!   2 and 20 minutes (efforts long enough to drain W', short enough that
//!   fatigue beyond W' depletion doesn't dominate; Skiba; Golden Cheetah's
//!   default range). Below that, neuromuscular power isn't W'-limited;
//!   above it, CP overestimates what can be held.
//! - **Which points.** Durations are sampled log-spaced (every ~10 %)
//!   within the range, so the fit isn't dominated by the long end, where
//!   one-second steps would put most of the points.
//! - **Max efforts only.** A curve from one easy ride is not a maximal
//!   power-duration relationship; the fit will be poor (low R²) and CP
//!   too low. Fit the season curve, or check R².

use alloc::vec::Vec;

use crate::mmp::PowerCurve;
use crate::num::{f64_from_usize, round_usize};

/// A fitted critical power model.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct CpFit {
    /// Critical power, W.
    pub cp: f64,
    /// W', J.
    pub w_prime: f64,
    /// 3-parameter model only: the time offset `k`, s (negative).
    pub k: Option<f64>,
    /// 3-parameter model only: predicted maximal power `CP − W'/k`, W.
    pub p_max: Option<f64>,
    /// Coefficient of determination of predicted vs. actual power.
    pub r_squared: f64,
    /// Root-mean-square error of predicted vs. actual power, W.
    pub rmse: f64,
    /// Standard error of CP, W (2-parameter model; for the 3-parameter
    /// model, conditional on the fitted `k`).
    pub se_cp: f64,
    /// Standard error of W', J (as for `se_cp`).
    pub se_w_prime: f64,
    /// Number of points fitted.
    pub points: usize,
}

impl CpFit {
    /// Power the model predicts can be held for `t` seconds, W.
    ///
    /// ```
    /// use zerofit_analytics::cp::fit_2p;
    /// let points: Vec<(f64, f64)> = [180.0, 300.0, 600.0, 1200.0]
    ///     .iter().map(|&t| (t, 20_000.0 / t + 280.0)).collect();
    /// let fit = fit_2p(&points).unwrap();
    /// assert!((fit.predict(400.0) - 330.0).abs() < 1e-6);
    /// ```
    #[must_use]
    pub fn predict(&self, t: f64) -> f64 {
        self.w_prime / (t - self.k.unwrap_or(0.0)) + self.cp
    }
}

/// Duration range and sampling for [`fit_curve_2p`] / [`fit_curve_3p`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitRange {
    /// Shortest duration, s.
    pub min_s: usize,
    /// Longest duration, s.
    pub max_s: usize,
}

impl FitRange {
    /// 2–20 minutes, the 2-parameter model's range.
    pub const TWO_PARAMETER: Self = Self {
        min_s: 120,
        max_s: 1200,
    };
    /// 10 s – 20 minutes, using sprint durations for the 3-parameter
    /// model's `Pmax`.
    pub const THREE_PARAMETER: Self = Self {
        min_s: 10,
        max_s: 1200,
    };

    /// Log-spaced durations within the range (each ~10 % longer than the
    /// last, deduplicated), clipped to `max_available`.
    #[must_use]
    pub fn durations(&self, max_available: usize) -> Vec<usize> {
        let max = self.max_s.min(max_available);
        let mut out: Vec<usize> = Vec::new();
        let mut t = f64_from_usize(self.min_s.max(1));
        loop {
            let d = round_usize(t);
            if d > max {
                break;
            }
            if out.last() != Some(&d) {
                out.push(d);
            }
            t *= 1.1;
        }
        if max >= self.min_s && out.last() != Some(&max) {
            out.push(max);
        }
        out
    }
}

/// Fits the 2-parameter model to `(t, P)` points (seconds, watts) by
/// least squares on the linear work-time form `P·t = CP·t + W'`
/// (Monod & Scherrer 1965).
///
/// `None` with fewer than 3 points, fewer than 2 distinct durations, or
/// a non-physical result (CP or W' not positive).
///
/// ```
/// use zerofit_analytics::cp::fit_2p;
/// // An exact hyperbola is recovered exactly.
/// let points: Vec<(f64, f64)> = [120.0, 300.0, 720.0, 1200.0]
///     .iter().map(|&t| (t, 18_000.0 / t + 250.0)).collect();
/// let fit = fit_2p(&points).unwrap();
/// assert!((fit.cp - 250.0).abs() < 1e-6 && (fit.w_prime - 18_000.0).abs() < 1e-3);
/// assert!(fit.r_squared > 0.999_999);
/// ```
#[must_use]
pub fn fit_2p(points: &[(f64, f64)]) -> Option<CpFit> {
    let xy: Vec<(f64, f64)> = points.iter().map(|&(t, p)| (t, p * t)).collect();
    let line = linear_fit(&xy)?;
    let (cp, w_prime) = (line.slope, line.intercept);
    if !(cp > 0.0 && w_prime > 0.0) {
        return None;
    }
    let (r_squared, rmse) = power_quality(points, |t| w_prime / t + cp);
    Some(CpFit {
        cp,
        w_prime,
        k: None,
        p_max: None,
        r_squared,
        rmse,
        se_cp: line.se_slope,
        se_w_prime: line.se_intercept,
        points: points.len(),
    })
}

/// Fits Morton's 3-parameter model to `(t, P)` points.
///
/// For a fixed `k`, `P = CP + W'·x` with `x = 1/(t − k)` is linear, so
/// CP and W' come from least squares; `k` is found by golden-section
/// search over `[−600 s, −0.01 s]` minimising the squared power error
/// (variable projection). Standard errors are conditional on that `k`.
///
/// `None` with fewer than 4 points or a non-physical result.
///
/// ```
/// use zerofit_analytics::cp::fit_3p;
/// let (cp, w, k) = (260.0, 20_000.0, -25.0);
/// let points: Vec<(f64, f64)> = [5.0, 15.0, 60.0, 180.0, 600.0, 1200.0]
///     .iter().map(|&t| (t, w / (t - k) + cp)).collect();
/// let fit = fit_3p(&points).unwrap();
/// assert!((fit.cp - cp).abs() < 0.1 && (fit.k.unwrap() - k).abs() < 0.1);
/// assert!((fit.p_max.unwrap() - (cp + w / 25.0)).abs() < 5.0);
/// ```
#[must_use]
pub fn fit_3p(points: &[(f64, f64)]) -> Option<CpFit> {
    if points.len() < 4 {
        return None;
    }
    let sse = |k: f64| -> f64 {
        fit_for_k(points, k).map_or(f64::INFINITY, |line| {
            points
                .iter()
                .map(|&(t, p)| {
                    let e = p - (line.intercept + line.slope / (t - k));
                    e * e
                })
                .sum()
        })
    };
    let k = golden_section_min(sse, -600.0, -0.01, 80);
    let line = fit_for_k(points, k)?;
    let (cp, w_prime) = (line.intercept, line.slope);
    if !(cp > 0.0 && w_prime > 0.0) {
        return None;
    }
    let (r_squared, rmse) = power_quality(points, |t| w_prime / (t - k) + cp);
    Some(CpFit {
        cp,
        w_prime,
        k: Some(k),
        p_max: Some(cp - w_prime / k),
        r_squared,
        rmse,
        se_cp: line.se_intercept,
        se_w_prime: line.se_slope,
        points: points.len(),
    })
}

/// [`fit_2p`] on log-spaced points of `curve` within `range`.
///
/// ```
/// use zerofit_analytics::{cp::{fit_curve_2p, FitRange}, mmp::PowerCurve};
/// assert!(fit_curve_2p(&PowerCurve::new(&[300; 60]), FitRange::TWO_PARAMETER).is_none());
/// ```
#[must_use]
pub fn fit_curve_2p(curve: &PowerCurve, range: FitRange) -> Option<CpFit> {
    fit_2p(&curve_points(curve, range))
}

/// [`fit_3p`] on log-spaced points of `curve` within `range`.
#[must_use]
pub fn fit_curve_3p(curve: &PowerCurve, range: FitRange) -> Option<CpFit> {
    fit_3p(&curve_points(curve, range))
}

fn curve_points(curve: &PowerCurve, range: FitRange) -> Vec<(f64, f64)> {
    range
        .durations(curve.max_duration())
        .into_iter()
        .filter_map(|d| Some((f64_from_usize(d), curve.watts(d)?)))
        .collect()
}

fn fit_for_k(points: &[(f64, f64)], k: f64) -> Option<Line> {
    let xy: Vec<(f64, f64)> = points.iter().map(|&(t, p)| (1.0 / (t - k), p)).collect();
    linear_fit(&xy)
}

/// R² and RMSE of `model(t)` against the points' power.
fn power_quality(points: &[(f64, f64)], model: impl Fn(f64) -> f64) -> (f64, f64) {
    let n = f64_from_usize(points.len());
    let mean = points.iter().map(|&(_, p)| p).sum::<f64>() / n;
    let (ss_res, ss_tot) = points.iter().fold((0.0, 0.0), |(r, tot), &(t, p)| {
        let e = p - model(t);
        (r + e * e, tot + (p - mean) * (p - mean))
    });
    let r_squared = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        1.0
    };
    (r_squared, libm::sqrt(ss_res / n))
}

#[derive(Debug, Clone, Copy)]
struct Line {
    slope: f64,
    intercept: f64,
    se_slope: f64,
    se_intercept: f64,
}

/// Ordinary least squares `y = slope·x + intercept`, with the textbook
/// standard errors `se_b = √(s²/Sxx)`, `se_a = √(s²·(1/n + x̄²/Sxx))`,
/// `s² = SSR/(n − 2)`.
fn linear_fit(xy: &[(f64, f64)]) -> Option<Line> {
    if xy.len() < 3 {
        return None;
    }
    let n = f64_from_usize(xy.len());
    let mx = xy.iter().map(|&(x, _)| x).sum::<f64>() / n;
    let my = xy.iter().map(|&(_, y)| y).sum::<f64>() / n;
    let (sxx, sxy) = xy.iter().fold((0.0, 0.0), |(sxx, sxy), &(x, y)| {
        (sxx + (x - mx) * (x - mx), sxy + (x - mx) * (y - my))
    });
    if sxx.is_nan() || sxx <= 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let ssr: f64 = xy
        .iter()
        .map(|&(x, y)| {
            let e = y - (slope * x + intercept);
            e * e
        })
        .sum();
    let s2 = ssr / (n - 2.0);
    Some(Line {
        slope,
        intercept,
        se_slope: libm::sqrt(s2 / sxx),
        se_intercept: libm::sqrt(s2 * (1.0 / n + mx * mx / sxx)),
    })
}

/// Minimum of a unimodal `f` on `[a, b]` by golden-section search.
fn golden_section_min(
    objective: impl Fn(f64) -> f64,
    mut lo: f64,
    mut hi: f64,
    iterations: u32,
) -> f64 {
    const INV_PHI: f64 = 0.618_033_988_749_894_8;
    let mut left = hi - (hi - lo) * INV_PHI;
    let mut right = lo + (hi - lo) * INV_PHI;
    let (mut f_left, mut f_right) = (objective(left), objective(right));
    for _ in 0..iterations {
        if f_left < f_right {
            hi = right;
            right = left;
            f_right = f_left;
            left = hi - (hi - lo) * INV_PHI;
            f_left = objective(left);
        } else {
            lo = left;
            left = right;
            f_left = f_right;
            right = lo + (hi - lo) * INV_PHI;
            f_right = objective(right);
        }
    }
    f64::midpoint(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn durations_are_log_spaced_and_clipped() {
        let d = FitRange::TWO_PARAMETER.durations(usize::MAX);
        assert_eq!(d.first(), Some(&120));
        assert_eq!(d.last(), Some(&1200));
        assert!(d.windows(2).all(|w| w[1] > w[0]));
        assert!(d.len() > 20 && d.len() < 30, "{}", d.len());
        let d = FitRange::TWO_PARAMETER.durations(500);
        assert_eq!(d.last(), Some(&500));
        assert_eq!(FitRange::TWO_PARAMETER.durations(60), [] as [usize; 0]);
    }

    #[test]
    fn recovers_cp_from_synthetic_curve() {
        // Build a "ride" whose MMP is exactly the 2-parameter hyperbola at
        // the fitted durations: P(t) = W'/t + CP sampled directly.
        let points: Vec<(f64, f64)> = FitRange::TWO_PARAMETER
            .durations(usize::MAX)
            .into_iter()
            .map(|d| {
                let t = f64_from_usize(d);
                (t, 22_000.0 / t + 300.0)
            })
            .collect();
        let fit = fit_2p(&points).unwrap();
        assert!((fit.cp - 300.0).abs() < 1e-6);
        assert!((fit.w_prime - 22_000.0).abs() < 1e-3);
        assert!(fit.rmse < 1e-6);
        assert!(fit.se_cp < 1e-6);
    }

    #[test]
    fn noisy_fit_reports_errors() {
        let points = vec![
            (120.0, 480.0),
            (300.0, 360.0),
            (600.0, 320.0),
            (1200.0, 302.0),
        ];
        let fit = fit_2p(&points).unwrap();
        assert!(fit.cp > 280.0 && fit.cp < 310.0, "{fit:?}");
        assert!(fit.rmse > 0.0 && fit.se_cp > 0.0 && fit.r_squared < 1.0);
    }

    #[test]
    fn rejects_degenerate_input() {
        assert!(fit_2p(&[(60.0, 300.0), (120.0, 280.0)]).is_none());
        assert!(fit_2p(&[(60.0, 300.0); 5]).is_none());
        // Power rising with duration: negative W'.
        assert!(fit_2p(&[(60.0, 200.0), (120.0, 250.0), (300.0, 300.0)]).is_none());
        assert!(fit_3p(&[(60.0, 300.0); 3]).is_none());
    }

    #[test]
    fn three_parameter_fit_from_curve() {
        // A ride with 10 s at 900 W, 60 s at 500 W, 300 s at 350 W,
        // 1200 s at 280 W blocks, separated by easy riding.
        let mut p = vec![100u16; 300];
        for (secs, watts) in [(10, 900u16), (60, 500), (300, 350), (1200, 280)] {
            p.extend(vec![watts; secs]);
            p.extend(vec![100u16; 300]);
        }
        let curve = PowerCurve::new(&p);
        let fit = fit_curve_3p(&curve, FitRange::THREE_PARAMETER).unwrap();
        // Block efforts are not a hyperbola, so the fit is approximate
        // (R² ≈ 0.92) but CP lands near the long efforts.
        assert!(fit.cp > 250.0 && fit.cp < 320.0, "{fit:?}");
        assert!(fit.r_squared > 0.8 && fit.r_squared < 0.99, "{fit:?}");
        assert!(fit.k.unwrap() < 0.0);
        let two = fit_curve_2p(&curve, FitRange::TWO_PARAMETER).unwrap();
        assert!(two.cp > 250.0 && two.cp < 320.0, "{two:?}");
    }
}
