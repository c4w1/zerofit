//! W' balance: how much of the anaerobic work capacity is left, second by
//! second (Skiba's differential model).
//!
//! ```text
//! P ≥ CP:  W'bal(t) = W'bal(t−1) − (P − CP)·Δt                 depletion
//! P < CP:  W'bal(t) = W' − (W' − W'bal(t−1)) · e^(−(CP − P)·Δt / W')   recovery
//! W'bal(0) = W'
//! ```
//!
//! Above CP, W' is spent joule for joule. Below CP, the *expended* part
//! `W' − W'bal` decays exponentially at a rate proportional to how far
//! below CP the rider is, so recovery is fast when soft-pedalling well
//! below CP and slow just under it. This is the differential form of
//! Skiba et al. (Skiba, Fulford, Clarke & Skiba 2015, *Eur J Appl
//! Physiol*; Clarke & Skiba 2013), which intervals.icu uses ("Intervals.icu
//! uses the differential algorithm", forum post by its developer, 2021).
//! It needs no time constant fitted from data, unlike Skiba's earlier
//! integral model (`τ = 546·e^(−0.01·D_CP) + 316`), and it is a single
//! O(n) pass.
//!
//! Judgment calls:
//!
//! - **W'bal may go negative.** That means the ride exceeded the model's
//!   W' (or CP is set too low); clamping at 0 would hide the evidence that
//!   the athlete settings need updating. intervals.icu shows negative
//!   values too.
//! - **It never exceeds W'.** Recovery approaches W' asymptotically, so
//!   no clamp is needed at the top; a property test checks this.
//! - **Pauses recover.** A removed pause of `Δt` seconds is treated as
//!   `Δt` seconds at 0 W (closed form `e^(−CP·Δt/W')`), because the
//!   athlete was resting, not frozen in time.

use alloc::vec::Vec;

use crate::ActivityStream;

/// W' balance, J, for each second of `power` (consecutive seconds).
///
/// Returns an empty vector if `cp` or `w_prime` is not positive.
///
/// ```
/// use zerofit_analytics::wbal::w_prime_balance;
/// // 60 s at 100 W above CP spends 6 kJ of a 20 kJ W'.
/// let bal = w_prime_balance(&[350; 60], 250.0, 20_000.0);
/// assert!((bal[59] - 14_000.0).abs() < 1e-9);
/// ```
#[must_use]
pub fn w_prime_balance(power: &[u16], cp: f64, w_prime: f64) -> Vec<f64> {
    let mut model = Model::new(cp, w_prime);
    match model {
        Some(ref mut m) => power.iter().map(|&p| m.step(f64::from(p), 1.0)).collect(),
        None => Vec::new(),
    }
}

/// W' balance, J, for each sample of `stream`, with paused time (the gaps
/// in [`ActivityStream::elapsed`]) treated as recovery at 0 W.
///
/// Returns an empty vector if `cp` or `w_prime` is not positive.
///
/// ```
/// use zerofit_analytics::{ActivityStream, stream::Sample, wbal::w_prime_balance_stream};
/// let mut s = ActivityStream::new(0);
/// s.push(Sample::power(0, 1250))?;   // 1 s at CP + 1000 W
/// s.push(Sample::power(600, 250))?;  // after a 599 s pause, at CP
/// let bal = w_prime_balance_stream(&s, 250.0, 20_000.0);
/// assert!((bal[0] - 19_000.0).abs() < 1e-9);
/// assert!(bal[1] > 19_999.0); // recovered during the pause
/// # Ok::<(), zerofit_analytics::stream::NotIncreasing>(())
/// ```
#[must_use]
pub fn w_prime_balance_stream(stream: &ActivityStream, cp: f64, w_prime: f64) -> Vec<f64> {
    let Some(mut model) = Model::new(cp, w_prime) else {
        return Vec::new();
    };
    let mut previous: Option<u32> = None;
    stream
        .power()
        .iter()
        .zip(stream.elapsed())
        .map(|(&p, &e)| {
            if let Some(prev) = previous {
                let paused = e.wrapping_sub(prev).saturating_sub(1);
                if paused > 0 {
                    model.step(0.0, f64::from(paused));
                }
            }
            previous = Some(e);
            model.step(f64::from(p), 1.0)
        })
        .collect()
}

/// The lowest W' balance, J, and the index where it occurred.
///
/// ```
/// use zerofit_analytics::wbal::min_balance;
/// assert_eq!(min_balance(&[20_000.0, 15_000.0, 18_000.0]), Some((1, 15_000.0)));
/// ```
#[must_use]
pub fn min_balance(balance: &[f64]) -> Option<(usize, f64)> {
    balance
        .iter()
        .copied()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

struct Model {
    cp: f64,
    w_prime: f64,
    balance: f64,
}

impl Model {
    fn new(cp: f64, w_prime: f64) -> Option<Self> {
        (cp > 0.0 && w_prime > 0.0 && cp.is_finite() && w_prime.is_finite()).then_some(Self {
            cp,
            w_prime,
            balance: w_prime,
        })
    }

    fn step(&mut self, power: f64, dt: f64) -> f64 {
        if power >= self.cp {
            self.balance -= (power - self.cp) * dt;
        } else {
            let expended = self.w_prime - self.balance;
            self.balance =
                self.w_prime - expended * libm::exp(-(self.cp - power) * dt / self.w_prime);
        }
        self.balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn hand_stepped_sequence() {
        // CP 200 W, W' 10 kJ. 3 s at 1200 W: −1000 J each.
        // Then 1 s at 100 W: expended 3000 J decays by e^(−100/10000).
        let bal = w_prime_balance(&[1200, 1200, 1200, 100], 200.0, 10_000.0);
        assert_eq!(&bal[..3], &[9000.0, 8000.0, 7000.0]);
        let expected = 10_000.0 - 3000.0 * libm::exp(-0.01);
        assert!((bal[3] - expected).abs() < 1e-9);
    }

    #[test]
    fn at_cp_nothing_changes() {
        let bal = w_prime_balance(&[200; 100], 200.0, 10_000.0);
        assert!(bal.iter().all(|&b| b == 10_000.0));
    }

    #[test]
    fn goes_negative_without_clamping() {
        let bal = w_prime_balance(&[1200; 20], 200.0, 10_000.0);
        assert_eq!(bal[19], -10_000.0);
        assert_eq!(min_balance(&bal), Some((19, -10_000.0)));
    }

    #[test]
    fn recovery_approaches_but_never_exceeds_w_prime() {
        let mut p = vec![600u16; 30];
        p.extend(vec![0u16; 100_000]);
        let bal = w_prime_balance(&p, 250.0, 20_000.0);
        assert!(bal.iter().all(|&b| b <= 20_000.0));
        assert!(bal.last().unwrap() > &19_999.999);
    }

    #[test]
    fn invalid_parameters() {
        assert_eq!(w_prime_balance(&[100], 0.0, 10_000.0), [] as [f64; 0]);
        assert_eq!(w_prime_balance(&[100], 200.0, -1.0), [] as [f64; 0]);
        assert_eq!(w_prime_balance(&[100], f64::NAN, 1.0), [] as [f64; 0]);
        assert_eq!(min_balance(&[]), None);
    }

    #[test]
    fn stream_without_pauses_matches_slice() {
        let p = [300u16, 500, 100, 0, 800];
        let s = ActivityStream::from_power(0, &p);
        assert_eq!(
            w_prime_balance_stream(&s, 250.0, 15_000.0),
            w_prime_balance(&p, 250.0, 15_000.0)
        );
    }
}
