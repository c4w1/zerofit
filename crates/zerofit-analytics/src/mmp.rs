//! Mean-maximal power (the power-duration curve).
//!
//! For each duration `d`, the best average power over any `d` consecutive
//! seconds:
//!
//! ```text
//! MMP(d) = max over i of (P[i] + … + P[i+d-1]) / d
//! ```
//!
//! # Algorithm
//!
//! With prefix sums `S[k] = P[0] + … + P[k-1]`, every window sum is one
//! subtraction, `S[i+d] − S[i]`, so a duration costs one pass of
//! `n − d + 1` subtract-and-max operations and the whole curve
//! `Σ_d (n − d + 1) ≈ n²/2`: about 233 million operations for a 6-hour
//! ride. That inner loop is two slices zipped together, branch-free and
//! with no bounds checks, so the compiler vectorizes it.
//!
//! The prefix sums are `u32` with wrapping arithmetic whenever the ride's
//! total work is below 2³² J (every ride under ~20 days at 2500 W):
//! wrapping differences are exact as long as every window sum fits, and
//! 32-bit lanes are twice as wide as 64-bit ones. Longer inputs fall
//! back to `u64`.
//!
//! **Why not faster?** Computing the maximum window sum for *every* window
//! length is an instance of (max,+) convolution, for which no truly
//! subquadratic algorithm is known; tools that are faster either compute a
//! subset of durations or approximate. [`mmp_at`] does the former in
//! O(n·k) for `k` chosen durations.
//!
//! # Monotonicity
//!
//! The exact curve never increases with duration: drop the lowest second
//! from the best `(d+1)`-second window, and what remains is a `d`-second
//! window whose average is at least as high. The sums here are exact
//! integers and `f64` division is correctly rounded (so it preserves
//! order), so the computed curve is non-increasing too, which a property
//! test checks.

use alloc::vec::Vec;

use crate::num::{f64_from_u64, f64_from_usize};

/// The best average power for every duration from 1 s to the length of the
/// input.
///
/// ```
/// use zerofit_analytics::mmp::PowerCurve;
/// let curve = PowerCurve::new(&[100, 400, 300, 100]);
/// assert_eq!(curve.watts(1), Some(400.0));
/// assert_eq!(curve.watts(2), Some(350.0));
/// assert_eq!(curve.watts(4), Some(225.0));
/// assert_eq!(curve.watts(5), None);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PowerCurve {
    /// `best_sum[d-1]` is the best `d`-second sum, J.
    best_sum: Vec<u64>,
}

impl PowerCurve {
    /// Computes the exact curve in O(n²/2) time and O(n) space; see the
    /// [module docs](self).
    #[must_use]
    pub fn new(power: &[u16]) -> Self {
        let total: u64 = power.iter().map(|&p| u64::from(p)).sum();
        let best_sum = if u32::try_from(total).is_ok() {
            best_sums::<u32>(power)
        } else {
            best_sums::<u64>(power)
        };
        Self { best_sum }
    }

    /// A curve from best sums per duration (`best_sum[d-1]` for `d`
    /// seconds), e.g. to rebuild a stored curve.
    #[must_use]
    pub fn from_best_sums(best_sum: Vec<u64>) -> Self {
        Self { best_sum }
    }

    /// Longest duration on the curve, s (the input length).
    #[must_use]
    pub fn max_duration(&self) -> usize {
        self.best_sum.len()
    }

    /// Whether the curve is empty (no input).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.best_sum.is_empty()
    }

    /// Best average power for `duration_s` seconds, W.
    #[must_use]
    pub fn watts(&self, duration_s: usize) -> Option<f64> {
        let sum = *self.best_sum.get(duration_s.checked_sub(1)?)?;
        Some(f64_from_u64(sum) / f64_from_usize(duration_s))
    }

    /// Best work for `duration_s` seconds, J.
    #[must_use]
    pub fn best_sum(&self, duration_s: usize) -> Option<u64> {
        self.best_sum.get(duration_s.checked_sub(1)?).copied()
    }

    /// `(duration_s, watts)` for every duration, shortest first.
    pub fn iter(&self) -> impl Iterator<Item = (usize, f64)> + '_ {
        (1usize..)
            .zip(&self.best_sum)
            .map(|(d, &s)| (d, f64_from_u64(s) / f64_from_usize(d)))
    }

    /// Raises every point to `other`'s where `other` is higher, and extends
    /// the curve to `other`'s length: the curve of the two activities
    /// together, as a season curve is built.
    ///
    /// Returns the durations where `other` set a new best.
    ///
    /// ```
    /// use zerofit_analytics::mmp::PowerCurve;
    /// let mut season = PowerCurve::new(&[300, 100]);
    /// let improved = season.merge_max(&PowerCurve::new(&[250, 250, 250]));
    /// assert_eq!(improved, vec![2, 3]);
    /// assert_eq!(season.watts(1), Some(300.0)); // kept
    /// assert_eq!(season.watts(2), Some(250.0)); // raised from 200
    /// assert_eq!(season.watts(3), Some(250.0)); // new duration
    /// ```
    pub fn merge_max(&mut self, other: &Self) -> Vec<usize> {
        let mut improved = Vec::new();
        for (d, (mine, theirs)) in (1usize..).zip(self.best_sum.iter_mut().zip(&other.best_sum)) {
            if *theirs > *mine {
                *mine = *theirs;
                improved.push(d);
            }
        }
        if let Some(rest) = other.best_sum.get(self.best_sum.len()..) {
            let first = self.best_sum.len().saturating_add(1);
            improved.extend((first..).take(rest.len()));
            self.best_sum.extend_from_slice(rest);
        }
        improved
    }
}

/// Unsigned integer used for prefix sums.
trait Acc: Copy + Ord + Default {
    fn from_u16(p: u16) -> Self;
    fn wrapping_add(self, other: Self) -> Self;
    fn wrapping_sub(self, other: Self) -> Self;
    fn to_u64(self) -> u64;
}

impl Acc for u32 {
    fn from_u16(p: u16) -> Self {
        Self::from(p)
    }
    fn wrapping_add(self, other: Self) -> Self {
        Self::wrapping_add(self, other)
    }
    fn wrapping_sub(self, other: Self) -> Self {
        Self::wrapping_sub(self, other)
    }
    fn to_u64(self) -> u64 {
        u64::from(self)
    }
}

impl Acc for u64 {
    fn from_u16(p: u16) -> Self {
        Self::from(p)
    }
    fn wrapping_add(self, other: Self) -> Self {
        Self::wrapping_add(self, other)
    }
    fn wrapping_sub(self, other: Self) -> Self {
        Self::wrapping_sub(self, other)
    }
    fn to_u64(self) -> u64 {
        self
    }
}

fn prefix_sums<T: Acc>(power: &[u16]) -> Vec<T> {
    let mut prefix = Vec::with_capacity(power.len().saturating_add(1));
    let mut acc = T::default();
    prefix.push(acc);
    for &p in power {
        acc = acc.wrapping_add(T::from_u16(p));
        prefix.push(acc);
    }
    prefix
}

/// The maximum of `S[i+d] − S[i]` over all `i`.
#[inline]
fn best_window<T: Acc>(prefix: &[T], d: usize) -> Option<T> {
    let ends = prefix.get(d..)?;
    ends.iter()
        .zip(prefix)
        .map(|(&e, &s)| e.wrapping_sub(s))
        .max()
}

fn best_sums<T: Acc>(power: &[u16]) -> Vec<u64> {
    let prefix = prefix_sums::<T>(power);
    (1..=power.len())
        .map(|d| best_window(&prefix, d).map_or(0, Acc::to_u64))
        .collect()
}

/// Best average power for selected durations only, in O(n·k) for `k`
/// durations: the right tool when only a handful of points are needed
/// (5 s, 1 min, 5 min, 20 min…). Durations longer than the input, or 0,
/// give `None`.
///
/// ```
/// use zerofit_analytics::mmp::mmp_at;
/// let power = [100, 400, 300, 100];
/// assert_eq!(mmp_at(&power, &[1, 3, 9]), vec![Some(400.0), Some(800.0 / 3.0), None]);
/// ```
#[must_use]
pub fn mmp_at(power: &[u16], durations: &[usize]) -> Vec<Option<f64>> {
    let prefix = prefix_sums::<u64>(power);
    durations
        .iter()
        .map(|&d| {
            if d == 0 {
                return None;
            }
            let sum = best_window(&prefix, d)?;
            Some(f64_from_u64(sum) / f64_from_usize(d))
        })
        .collect()
}

/// Durations used for summaries and the CP fit: 1 s … 6 h, roughly
/// log-spaced, s.
pub const STANDARD_DURATIONS: [usize; 24] = [
    1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 300, 420, 600, 900, 1200, 1800, 2700, 3600,
    5400, 7200, 21600,
];

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// Brute force: every window of every length.
    fn naive(power: &[u16]) -> Vec<f64> {
        (1..=power.len())
            .map(|d| {
                power
                    .windows(d)
                    .map(|w| w.iter().map(|&p| f64::from(p)).sum::<f64>() / f64_from_usize(d))
                    .fold(f64::MIN, f64::max)
            })
            .collect()
    }

    #[test]
    fn matches_brute_force() {
        let mut seed = 12345u32;
        let power: Vec<u16> = (0..300)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                u16::try_from((seed >> 16) % 1200).unwrap()
            })
            .collect();
        let curve = PowerCurve::new(&power);
        for ((d, got), want) in curve.iter().zip(naive(&power)) {
            assert!((got - want).abs() < 1e-9, "d={d}: {got} vs {want}");
        }
    }

    #[test]
    fn u64_path_matches_u32_path() {
        let power = [65535u16, 1, 65535, 7, 300];
        let a = best_sums::<u32>(&power);
        let b = best_sums::<u64>(&power);
        assert_eq!(a, b);
    }

    #[test]
    fn huge_totals_are_exact() {
        // 70k seconds at 65535 W: the total exceeds 2^32, so a u32 sum
        // would wrap. (The full curve of this input is 2.5e9 operations,
        // too slow for a debug-mode unit test; mmp_at uses u64 too.)
        let power = vec![65535u16; 70_000];
        assert_eq!(mmp_at(&power, &[70_000]), vec![Some(65535.0)]);
    }

    #[test]
    fn empty_and_edges() {
        let curve = PowerCurve::new(&[]);
        assert!(curve.is_empty());
        assert_eq!(curve.watts(1), None);
        assert_eq!(curve.watts(0), None);
        assert_eq!(mmp_at(&[], &[1]), vec![None]);
        assert_eq!(mmp_at(&[5], &[0, 1]), vec![None, Some(5.0)]);
    }

    #[test]
    fn constant_power_flat_curve() {
        let curve = PowerCurve::new(&[250; 100]);
        assert!(curve.iter().all(|(_, w)| w == 250.0));
    }

    #[test]
    fn merge_extends() {
        let mut a = PowerCurve::default();
        let improved = a.merge_max(&PowerCurve::new(&[10, 20]));
        assert_eq!(improved, vec![1, 2]);
        assert_eq!(a.max_duration(), 2);
    }
}
