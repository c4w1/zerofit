//! Property tests: invariants that must hold for every input.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::float_cmp
)]

use proptest::prelude::*;
use zerofit_analytics::mmp::PowerCurve;
use zerofit_analytics::power::{average_power, normalized_power, tss};
use zerofit_analytics::resample::{RawRecord, ResampleConfig, resample};
use zerofit_analytics::wbal::w_prime_balance;
use zerofit_analytics::{ActivityStream, AnalysisConfig, AthleteSettings, analyze_stream};

fn power_stream(max_len: usize) -> impl Strategy<Value = Vec<u16>> {
    prop::collection::vec(0u16..1500, 30..max_len)
}

/// Mean of the 30 s rolling averages, the quantity NP is a power mean of.
fn mean_rolling(p: &[u16]) -> f64 {
    let windows: Vec<f64> = p
        .windows(30)
        .map(|w| w.iter().map(|&x| f64::from(x)).sum::<f64>() / 30.0)
        .collect();
    windows.iter().sum::<f64>() / windows.len() as f64
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// NP is the 4-power mean of the rolling averages, so it is at least
    /// their arithmetic mean (power-mean inequality), for any input.
    #[test]
    fn np_at_least_mean_of_rolling_averages(p in power_stream(600)) {
        let np = normalized_power(&p).unwrap();
        prop_assert!(np >= mean_rolling(&p) * (1.0 - 1e-12));
    }

    /// NP ≥ average power when the ride starts and ends with 29 s of zeros
    /// (every non-zero sample then sits in a full 30 windows). Without the
    /// padding it can fail at the edges: see `np_below_average_at_edges`.
    #[test]
    fn np_at_least_average_with_zero_edges(core in prop::collection::vec(0u16..1500, 1..600)) {
        let mut p = vec![0u16; 29];
        p.extend(&core);
        p.extend([0u16; 29]);
        let np = normalized_power(&p).unwrap();
        let avg = average_power(&p).unwrap();
        prop_assert!(np >= avg * (1.0 - 1e-12), "np {np} avg {avg}");
    }

    /// What the exact curve does guarantee (it is *not* non-increasing in
    /// general, see `mmp_can_increase`): best work never decreases with
    /// duration, MMP(k·d) ≤ MMP(d), MMP(1) is the max and MMP(n) the
    /// average. The envelope is non-increasing and never below the curve.
    #[test]
    fn mmp_invariants(p in prop::collection::vec(0u16..2000, 1..400)) {
        let curve = PowerCurve::new(&p);
        let n = p.len();
        let sums: Vec<u64> = (1..=n).map(|d| curve.best_sum(d).unwrap()).collect();
        prop_assert!(sums.windows(2).all(|w| w[0] <= w[1]));
        for d in 1..=n {
            for k in 2..=(n / d) {
                prop_assert!(curve.watts(k * d).unwrap() <= curve.watts(d).unwrap());
            }
        }
        prop_assert_eq!(curve.watts(1).unwrap(), f64::from(*p.iter().max().unwrap()));
        prop_assert!((curve.watts(n).unwrap() - average_power(&p).unwrap()).abs() < 1e-9);
        let env = curve.envelope();
        prop_assert!(env.windows(2).all(|w| w[1] <= w[0]));
        prop_assert!(curve.iter().zip(&env).all(|((_, w), e)| *e >= w));
    }

    /// W'bal never exceeds W', whatever the power and parameters.
    #[test]
    fn w_prime_balance_never_exceeds_w_prime(
        p in prop::collection::vec(0u16..2000, 0..2000),
        cp in 100.0f64..400.0,
        w in 5_000.0f64..40_000.0,
    ) {
        let bal = w_prime_balance(&p, cp, w);
        prop_assert_eq!(bal.len(), p.len());
        prop_assert!(bal.iter().all(|&b| b <= w));
    }

    /// TSS of steady riding is exactly linear in duration.
    #[test]
    fn tss_scales_with_duration_at_constant_power(
        watts in 1u16..600, secs in 30usize..4000, k in 1usize..5, ftp in 150.0f64..400.0,
    ) {
        let one = ActivityStream::from_power(0, &vec![watts; secs]);
        let many = ActivityStream::from_power(0, &vec![watts; secs * k]);
        let t1 = tss(secs as f64, normalized_power(one.power()).unwrap(), ftp).unwrap();
        let tk = tss((secs * k) as f64, normalized_power(many.power()).unwrap(), ftp).unwrap();
        prop_assert!((tk - k as f64 * t1).abs() < 1e-6 * tk.max(1.0));
    }

    /// Riding on never lowers TSS (recording duration): appending any
    /// non-negative power adds non-negative 4th powers to NP's sum while
    /// t²/(t − 29) grows.
    #[test]
    fn tss_never_decreases_when_riding_continues(
        p in power_stream(500), more in prop::collection::vec(0u16..1500, 1..500),
    ) {
        let before = tss(p.len() as f64, normalized_power(&p).unwrap(), 250.0).unwrap();
        let mut longer = p.clone();
        longer.extend(&more);
        let after = tss(longer.len() as f64, normalized_power(&longer).unwrap(), 250.0).unwrap();
        prop_assert!(after >= before * (1.0 - 1e-12), "{before} -> {after}");
    }

    /// The resampler never panics, emits strictly increasing elapsed
    /// times, and accounts for every second it keeps.
    #[test]
    fn resampler_accounting(
        records in prop::collection::vec(
            (0u32..500, prop::option::of(0u16..3000), prop::option::of(40u8..200)),
            0..300,
        ),
    ) {
        let records: Vec<RawRecord> = records
            .into_iter()
            .map(|(t, p, h)| RawRecord { power: p, heart_rate: h, ..RawRecord::at(t) })
            .collect();
        let (s, r) = resample(&records, &[], &ResampleConfig::default());
        prop_assert!(s.elapsed().windows(2).all(|w| w[0] < w[1]));
        let kept = r.records - r.duplicates - r.out_of_order;
        prop_assert_eq!(s.len(), kept + r.gap_seconds_filled);
        if let (Some(first), Some(last)) = (s.elapsed().first(), s.elapsed().last()) {
            prop_assert_eq!(*first, 0);
            prop_assert_eq!((last + 1) as usize, s.len() + r.paused_seconds);
        }
    }

    /// The full pipeline never panics and keeps NP/IF/TSS consistent.
    #[test]
    fn analysis_is_consistent(p in power_stream(900)) {
        let stream = ActivityStream::from_power(0, &p);
        let athlete = AthleteSettings { ftp: Some(250.0), ..AthleteSettings::default() };
        let s = analyze_stream(&stream, &athlete, &AnalysisConfig::default()).summary;
        let (np, if_) = (s.normalized_power.unwrap(), s.intensity_factor.unwrap());
        prop_assert!((if_ - np / 250.0).abs() < 1e-12);
        let expected_tss = p.len() as f64 / 3600.0 * if_ * if_ * 100.0;
        prop_assert!((s.tss.unwrap() - expected_tss).abs() < 1e-9 * expected_tss.max(1.0));
    }
}

/// The requested invariant "MMP is non-increasing" is false for the exact
/// curve; this is the minimal counterexample proptest found.
#[test]
fn mmp_can_increase() {
    let curve = PowerCurve::new(&[980, 0, 980]);
    assert_eq!(curve.watts(2), Some(490.0));
    assert!(curve.watts(3).unwrap() > 650.0);
}

/// The edge case that makes "NP ≥ average" a padded-only invariant: one
/// hard second at the very start sits in a single 30 s window.
#[test]
fn np_below_average_at_edges() {
    let mut p = vec![1000u16];
    p.extend([0u16; 30]);
    let np = normalized_power(&p).unwrap();
    let avg = average_power(&p).unwrap();
    assert!(np < avg, "np {np} avg {avg}");
}
