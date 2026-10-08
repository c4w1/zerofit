//! The FIT workout export, decoded back with zerofit and (when
//! `FIT_CSV_TOOL` points at the FIT SDK's FitCSVTool.jar) with Garmin's
//! own tool, which names every field: the strongest check that the
//! hand-written profile numbers are right.
#![cfg(feature = "fit")]
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use zerofit::{Decoder, Record, Value};
use zerofit_analytics::workout::fit_workout::{FILE_ID, WORKOUT, WORKOUT_STEP};
use zerofit_analytics::workout::{StepKind, Workout, WorkoutStep};

fn sample() -> Workout {
    Workout {
        name: "Sweet spot 2x20".into(),
        steps: vec![
            WorkoutStep::ramp(600, 50.0, 75.0, StepKind::Warmup),
            WorkoutStep::steady(1200, 90.0, StepKind::Active),
            WorkoutStep::steady(300, 50.0, StepKind::Recovery),
            WorkoutStep::steady(1200, 90.0, StepKind::Active),
            WorkoutStep::ramp(600, 60.0, 40.0, StepKind::Cooldown),
        ],
    }
}

fn uint(v: Option<Value<'_>>) -> u64 {
    match v.unwrap() {
        Value::UInt8(x) | Value::Enum(x) => u64::from(x),
        Value::UInt16(x) => u64::from(x),
        Value::UInt32(x) => u64::from(x),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn decodes_back_with_zerofit() {
    let bytes = sample().to_fit(1_100_000_000).unwrap();
    let mut globals = Vec::new();
    let mut steps = Vec::new();
    for record in Decoder::new(&bytes) {
        let Record::Data(msg) = record.unwrap() else {
            continue;
        };
        globals.push(msg.global_message_number());
        let f = |n| msg.field(n).and_then(|f| f.value());
        match msg.global_message_number() {
            FILE_ID => {
                assert_eq!(uint(f(0)), 5);
                assert_eq!(uint(f(4)), 1_100_000_000);
            }
            WORKOUT => {
                assert_eq!(uint(f(4)), 2);
                assert_eq!(uint(f(6)), 5);
                let Some(Value::String(name)) = f(8) else {
                    panic!("name")
                };
                assert_eq!(name.to_str(), Ok("Sweet spot 2x20"));
            }
            WORKOUT_STEP => {
                steps.push((uint(f(254)), uint(f(2)), uint(f(5)), uint(f(6)), uint(f(7))));
            }
            _ => {}
        }
    }
    assert_eq!(globals, [0, 26, 27, 27, 27, 27, 27]);
    assert_eq!(
        steps,
        [
            (0, 600_000, 50, 75, 2),
            (1, 1_200_000, 88, 93, 0),
            (2, 300_000, 48, 53, 1),
            (3, 1_200_000, 88, 93, 0),
            (4, 600_000, 40, 60, 3),
        ]
    );
}

#[test]
fn long_and_unicode_names_are_truncated_on_char_boundaries() {
    let w = Workout {
        name: "Überlange Einheit mit sehr vielen Wörtern äöü".into(),
        steps: vec![WorkoutStep::steady(60, 100.0, StepKind::Active)],
    };
    let bytes = w.to_fit(0).unwrap();
    assert!(Decoder::new(&bytes).all(|r| r.is_ok()));
}

/// FitCSVTool names every message and field we wrote and reads back the
/// same values. Skipped unless `FIT_CSV_TOOL` is set (needs java).
#[test]
fn fitcsvtool_reads_the_workout() {
    let Ok(jar) = std::env::var("FIT_CSV_TOOL") else {
        eprintln!("FIT_CSV_TOOL not set; skipping FitCSVTool check");
        return;
    };
    let dir = std::env::temp_dir().join("zerofit-workout-fitcsv");
    std::fs::create_dir_all(&dir).unwrap();
    let fit = dir.join("workout.fit");
    std::fs::write(&fit, sample().to_fit(1_100_000_000).unwrap()).unwrap();
    let csv = dir.join("workout.csv");
    let status = std::process::Command::new("java")
        .args(["-jar", &jar, "-b"])
        .arg(&fit)
        .arg(&csv)
        .status()
        .expect("run java");
    assert!(status.success());
    let text = std::fs::read_to_string(&csv).unwrap();
    // FitCSVTool prints raw enum values; that it resolves the subfields
    // `duration_time` and `custom_target_power_*` proves duration_type 0 is
    // time and target_type 4 is power.
    for needle in [
        "Data,0,file_id,type,\"5\"",
        "workout,sport,\"2\",,num_valid_steps,\"5\",,wkt_name,\"Sweet spot 2x20\"",
        "message_index,\"0\",,duration_type,\"0\",,duration_time,\"600.0\",,target_type,\"4\",,target_power_zone,\"0\",,custom_target_power_low,\"50\",,custom_target_power_high,\"75\",,intensity,\"2\"",
        "duration_time,\"1200.0\"",
        "custom_target_power_low,\"88\",,custom_target_power_high,\"93\",,intensity,\"0\"",
        "custom_target_power_low,\"48\",,custom_target_power_high,\"53\",,intensity,\"1\"",
        "custom_target_power_low,\"40\",,custom_target_power_high,\"60\",,intensity,\"3\"",
    ] {
        assert!(text.contains(needle), "{needle} not in:\n{text}");
    }
}
