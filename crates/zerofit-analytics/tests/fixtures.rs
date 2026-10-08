//! zerofit-analytics against intervals.icu on the real fixture rides.
//!
//! Two references, both independent of this crate:
//!
//! 1. `tests/fixtures/intervals_icu.json`: values recorded by hand from
//!    the intervals.icu web app. `null` entries are pending and skipped.
//! 2. The `session` message intervals.icu writes into its own FIT export
//!    (`file_id.product_name = "Intervals.icu"`): average power, NP, IF,
//!    TSS, total work and the FTP it used. These are intervals.icu's
//!    numbers: the export's `total_work` equals the work of our resampled
//!    stream to the joule on `icu_laps`.
//!
//! Run with `--nocapture` to see the error tables. The FIT files live in
//! `crates/zerofit/tests/fixtures/`; the tests skip if they are missing
//! (e.g. in the published package).
#![cfg(feature = "fit")]
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

use std::path::{Path, PathBuf};

use serde_json::Value;
use zerofit_analytics::fit::read_fit;
use zerofit_analytics::{ActivitySummary, AnalysisConfig, AthleteSettings, analyze_fit};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zerofit/tests/fixtures")
}

fn load(name: &str) -> Option<Vec<u8>> {
    std::fs::read(fixture_dir().join(format!("{name}.fit"))).ok()
}

/// How a metric is compared.
#[derive(Clone, Copy)]
enum Tol {
    /// |ours − theirs| / |theirs| ≤ r.
    Rel(f64),
    /// |ours − theirs| ≤ a.
    Abs(f64),
}

impl Tol {
    fn ok(self, ours: f64, theirs: f64) -> bool {
        match self {
            Self::Rel(r) => (ours - theirs).abs() <= r * theirs.abs().max(1e-9),
            Self::Abs(a) => (ours - theirs).abs() <= a,
        }
    }
    fn show(self) -> String {
        match self {
            Self::Rel(r) => format!("±{:.1}%", r * 100.0),
            Self::Abs(a) => format!("±{a}"),
        }
    }
}

struct Row {
    ride: String,
    metric: String,
    ours: Option<f64>,
    theirs: f64,
    tol: Tol,
}

impl Row {
    fn pass(&self) -> bool {
        self.ours.is_some_and(|o| self.tol.ok(o, self.theirs))
    }
}

fn print_table(title: &str, rows: &[Row]) {
    eprintln!("\n{title}\n| ride | metric | zerofit-analytics | reference | error | tolerance | |");
    eprintln!("|---|---|---|---|---|---|---|");
    for r in rows {
        let (ours, err) = match r.ours {
            Some(o) => {
                let rel = if r.theirs == 0.0 {
                    String::new()
                } else {
                    format!(" ({:+.2}%)", (o - r.theirs) / r.theirs * 100.0)
                };
                (format!("{o:.3}"), format!("{:+.3}{rel}", o - r.theirs))
            }
            None => ("—".into(), "missing".into()),
        };
        eprintln!(
            "| {} | {} | {ours} | {} | {err} | {} | {} |",
            r.ride,
            r.metric,
            r.theirs,
            r.tol.show(),
            if r.pass() { "ok" } else { "FAIL" }
        );
    }
}

fn settings_from(v: &Value) -> AthleteSettings {
    let f = |k: &str| v.get(k).and_then(Value::as_f64);
    let u8_ = |k: &str| f(k).map(|x| x as u8);
    AthleteSettings {
        ftp: f("ftp_used"),
        weight_kg: f("weight_kg"),
        lthr: u8_("lthr"),
        max_hr: u8_("max_hr"),
        resting_hr: u8_("resting_hr"),
        cp: f("cp"),
        w_prime: f("w_prime_j"),
        ..AthleteSettings::default()
    }
}

/// Scalar metrics: (json key, accessor, tolerance).
type Getter = fn(&ActivitySummary) -> Option<f64>;
const SCALARS: &[(&str, Getter, Tol)] = &[
    (
        "moving_time_s",
        |s| Some(s.moving_time_s as f64),
        Tol::Rel(0.01),
    ),
    (
        "elapsed_time_s",
        |s| Some(f64::from(s.elapsed_time_s)),
        Tol::Abs(2.0),
    ),
    ("average_power", |s| s.average_power, Tol::Rel(0.01)),
    ("normalized_power", |s| s.normalized_power, Tol::Rel(0.01)),
    ("intensity_factor", |s| s.intensity_factor, Tol::Rel(0.01)),
    ("tss", |s| s.tss, Tol::Rel(0.02)),
    ("variability_index", |s| s.variability_index, Tol::Rel(0.01)),
    ("work_kj", |s| s.work_kj, Tol::Rel(0.01)),
    ("average_hr", |s| s.average_hr, Tol::Abs(1.0)),
    ("hr_tss", |s| s.hr_tss, Tol::Rel(0.05)),
    ("efficiency_factor", |s| s.efficiency_factor, Tol::Rel(0.01)),
    ("decoupling_pct", |s| s.decoupling_pct, Tol::Abs(1.0)),
    ("eftp", |s| s.eftp.map(|e| e.eftp), Tol::Rel(0.03)),
    (
        "min_w_prime_balance_kj",
        |s| s.min_w_prime_balance.map(|b| b / 1000.0),
        Tol::Abs(1.0),
    ),
];

#[test]
fn matches_values_recorded_from_intervals_icu() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/intervals_icu.json");
    let json: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut rows = Vec::new();
    let mut pending = 0usize;
    for (ride, entry) in json["rides"].as_object().unwrap() {
        let Some(bytes) = load(ride) else {
            eprintln!("skipping {ride}: fixture not found");
            continue;
        };
        let athlete = settings_from(&entry["settings"]);
        let theirs = &entry["intervals_icu"];
        let s = analyze_fit(&bytes, &athlete, &AnalysisConfig::default())
            .unwrap()
            .summary;
        for (key, get, tol) in SCALARS {
            match theirs.get(*key).and_then(Value::as_f64) {
                Some(t) => rows.push(Row {
                    ride: ride.clone(),
                    metric: (*key).into(),
                    ours: get(&s),
                    theirs: t,
                    tol: *tol,
                }),
                None => pending += 1,
            }
        }
        for (dur, t) in theirs["mmp_watts"].as_object().unwrap() {
            let Some(t) = t.as_f64() else {
                pending += 1;
                continue;
            };
            let d: usize = dur.parse().unwrap();
            rows.push(Row {
                ride: ride.clone(),
                metric: format!("mmp {d} s"),
                ours: s
                    .power_curve
                    .iter()
                    .find(|p| p.duration_s == d)
                    .map(|p| p.watts),
                theirs: t,
                tol: Tol::Rel(0.01),
            });
        }
        for (key, ours) in [
            ("power_zone_seconds", &s.power_zone_seconds),
            ("hr_zone_seconds", &s.hr_zone_seconds),
        ] {
            let Some(zones) = theirs[key].as_array() else {
                pending += 1;
                continue;
            };
            let total: f64 = zones.iter().filter_map(Value::as_f64).sum();
            for (i, t) in zones.iter().enumerate() {
                rows.push(Row {
                    ride: ride.clone(),
                    metric: format!("{key} Z{}", i + 1),
                    ours: ours.as_ref().and_then(|z| z.get(i)).map(|&x| f64::from(x)),
                    theirs: t.as_f64().unwrap(),
                    tol: Tol::Abs((total * 0.02).max(30.0)),
                });
            }
        }
    }
    print_table("Recorded intervals.icu values", &rows);
    eprintln!(
        "{} compared, {pending} pending (null in intervals_icu.json)",
        rows.len()
    );
    let failures: Vec<_> = rows
        .iter()
        .filter(|r| !r.pass())
        .map(|r| &r.metric)
        .collect();
    assert!(failures.is_empty(), "outside tolerance: {failures:?}");
}

/// The session values intervals.icu wrote into its FIT exports, with the
/// FTP it used (`threshold_power`).
///
/// Tolerances and the known differences (see the crate README):
/// - average power: recording time differs by a few seconds (intervals.icu
///   counts (P + 2) fewer seconds for P pauses), ~0.1 %.
/// - NP: matches `icu_intervals` to 0.05 %; `icu_laps` is 0.8 % low and
///   no NP variant tried (per-segment windows, partial windows, EWMA,
///   excluding zeros) explains it without breaking the other ride.
/// - TSS: intervals.icu uses moving time with an unpublished speed
///   threshold; ours (0.5 m/s) gives a 0.7 % and 1.5 % difference, all
///   of it in the duration (NP and IF match).
#[test]
fn matches_session_values_written_by_intervals_icu() {
    let mut rows = Vec::new();
    for ride in ["icu_laps", "icu_intervals"] {
        let Some(bytes) = load(ride) else {
            eprintln!("skipping {ride}: fixture not found");
            continue;
        };
        let session = read_fit(&bytes).unwrap().session.unwrap();
        let ftp = f64::from(session.threshold_power.unwrap());
        let athlete = AthleteSettings {
            ftp: Some(ftp),
            ..AthleteSettings::default()
        };
        let s = analyze_fit(&bytes, &athlete, &AnalysisConfig::default())
            .unwrap()
            .summary;
        let mut push = |metric: &str, ours: Option<f64>, theirs: Option<f64>, tol| {
            rows.push(Row {
                ride: ride.into(),
                metric: metric.into(),
                ours,
                theirs: theirs.unwrap(),
                tol,
            });
        };
        // The export stores whole watts and three-decimal IF.
        push(
            "average_power",
            s.average_power,
            session.avg_power.map(f64::from),
            Tol::Abs(0.6),
        );
        push(
            "normalized_power",
            s.normalized_power,
            session.normalized_power.map(f64::from),
            Tol::Rel(0.01),
        );
        push(
            "intensity_factor",
            s.intensity_factor,
            session.intensity_factor,
            Tol::Rel(0.01),
        );
        push("tss", s.tss, session.training_stress_score, Tol::Rel(0.02));
        push(
            "work_kj",
            s.work_kj,
            session.total_work.map(|j| f64::from(j) / 1000.0),
            Tol::Rel(0.0001),
        );
    }
    print_table("Session values in intervals.icu's FIT export", &rows);
    let failures: Vec<_> = rows
        .iter()
        .filter(|r| !r.pass())
        .map(|r| format!("{} {}", r.ride, r.metric))
        .collect();
    assert!(failures.is_empty(), "outside tolerance: {failures:?}");
}

/// The Wahoo head unit's own session values, for information only: a
/// device is not a reference implementation (its `total_work` is below
/// the sum of its own per-second power records), so nothing is asserted.
#[test]
fn wahoo_device_values_for_information() {
    let Some(bytes) = load("wahoo_elemnt") else {
        return;
    };
    let session = read_fit(&bytes).unwrap().session.unwrap();
    let athlete = AthleteSettings {
        ftp: session.threshold_power.map(f64::from),
        ..AthleteSettings::default()
    };
    let s = analyze_fit(&bytes, &athlete, &AnalysisConfig::default())
        .unwrap()
        .summary;
    eprintln!(
        "wahoo_elemnt: avg {:?} vs {:?}, NP {:?} vs {:?}, TSS {:?} vs {:?}",
        s.average_power,
        session.avg_power,
        s.normalized_power,
        session.normalized_power,
        s.tss,
        session.training_stress_score
    );
    assert!(s.average_power.is_some());
}

/// Every fixture analyzes without error and produces sane values; prints
/// a few headline numbers and how often, and by how much, the exact MMP
/// curve rises with duration (it does; see the `mmp` module docs).
#[test]
fn every_fixture_analyzes() {
    for ride in ["icu_short", "icu_laps", "icu_intervals", "wahoo_elemnt"] {
        let Some(bytes) = load(ride) else { continue };
        let athlete = AthleteSettings {
            ftp: Some(250.0),
            ..AthleteSettings::default()
        };
        let a = analyze_fit(&bytes, &athlete, &AnalysisConfig::default()).unwrap();
        let steps: Vec<(usize, f64)> = a
            .power_curve
            .iter()
            .zip(a.power_curve.iter().skip(1))
            .filter(|((_, w1), (_, w2))| w2 > w1)
            .map(|((d, w1), (_, w2))| (d, w2 - w1))
            .collect();
        let rises = steps.len();
        let biggest = steps
            .iter()
            .copied()
            .fold((0, 0.0), |m, x| if x.1 > m.1 { x } else { m });
        let env = a.power_curve.envelope();
        let max_gap = a
            .power_curve
            .iter()
            .zip(&env)
            .map(|((_, w), e)| (e - w) / e)
            .fold(0.0f64, f64::max);
        eprintln!(
            "{ride}: {} s recorded, NP {:?}, curve {} s, MMP rises at {rises} durations (largest              {:.3} W after {} s; curve at most {:.3}% below its envelope), report {:?}",
            a.summary.recording_time_s,
            a.summary.normalized_power.map(f64::round),
            a.power_curve.max_duration(),
            biggest.1,
            biggest.0,
            max_gap * 100.0,
            a.summary.resample_report
        );
        assert!(a.summary.recording_time_s > 0);
    }
}
