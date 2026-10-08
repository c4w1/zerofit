//! zerofit-analytics throughput on long synthetic inputs.
//!
//! ```sh
//! cargo bench -p zerofit-bench --bench analytics
//! ```
//!
//! - `analysis/full_4h`: resample 4 h of raw records and compute every
//!   metric (NP, TSS, zones, hrTSS, decoupling, full MMP curve, CP fit,
//!   eFTP, W'bal), i.e. `analyze_records`.
//! - `mmp/curve_6h`: the exact power curve, every duration 1 s–6 h.
//! - `mmp/standard_durations_6h`: 24 selected durations only (`mmp_at`).
//! - `load/ctl_atl_3y`: CTL/ATL/TSB over 1095 days.
//! - `analysis/fixture/<name>`: `analyze_fit` on each real fixture file.

#![allow(missing_docs)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use zerofit_analytics::load::{LoadConfig, training_load};
use zerofit_analytics::mmp::{PowerCurve, STANDARD_DURATIONS, mmp_at};
use zerofit_analytics::resample::{ResampleConfig, resample};
use zerofit_analytics::{AnalysisConfig, AthleteSettings, analyze_fit, analyze_records};
use zerofit_bench as w;

fn athlete() -> AthleteSettings {
    AthleteSettings {
        ftp: Some(260.0),
        weight_kg: Some(72.0),
        lthr: Some(165),
        max_hr: Some(188),
        resting_hr: Some(48),
        ..AthleteSettings::default()
    }
}

fn bench(c: &mut Criterion) {
    let config = AnalysisConfig::default();
    let ride_4h = w::synthetic_ride(4 * 3600, 1);
    let mut g = c.benchmark_group("analysis");
    g.sample_size(20);
    g.bench_function("full_4h", |b| {
        b.iter(|| analyze_records(black_box(&ride_4h), &[], &athlete(), &config));
    });
    for f in w::fixtures() {
        g.bench_function(format!("fixture/{}", f.name), |b| {
            b.iter(|| analyze_fit(black_box(&f.bytes), &athlete(), &config));
        });
    }
    g.finish();

    let (stream_6h, _) = resample(
        &w::synthetic_ride(6 * 3600, 2),
        &[],
        &ResampleConfig::default(),
    );
    let power_6h = stream_6h.power_elapsed();
    assert_eq!(power_6h.len(), 6 * 3600);
    let mut g = c.benchmark_group("mmp");
    g.sample_size(20);
    g.bench_function("curve_6h", |b| {
        b.iter(|| PowerCurve::new(black_box(&power_6h)));
    });
    g.bench_function("standard_durations_6h", |b| {
        b.iter(|| mmp_at(black_box(&power_6h), &STANDARD_DURATIONS));
    });
    g.finish();

    let loads = w::synthetic_daily_loads(3 * 365, 3);
    c.bench_function("load/ctl_atl_3y", |b| {
        b.iter(|| training_load(black_box(&loads), &LoadConfig::default()));
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
