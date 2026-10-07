//! The profile layer against FitCSVTool ground truth for every fixture in
//! `crates/zerofit/tests/fixtures/` (see that crate's `tests/fixtures.rs`).

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_precision_loss
)]

#[path = "../../zerofit/tests/common/expected.rs"]
mod expected;

use expected::{data_messages, load_fixtures, nth_message};
use zerofit_profile::messages::Session;
use zerofit_profile::{Message, message_info};

/// Every scaled session value FitCSVTool printed, through the generated
/// field metadata.
#[test]
fn session_values_match_fitcsvtool() {
    let info = message_info(Session::GLOBAL).unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for f in load_fixtures() {
        let messages = data_messages(&f.bytes);
        let Some(session) = nth_message(&messages, Session::GLOBAL, 0) else {
            assert!(f.expected.profile.session.is_empty(), "{}", f.name);
            continue;
        };
        for (name, want) in &f.expected.profile.session {
            let field = info.fields().iter().find(|fi| fi.name() == name).unwrap();
            let got = session
                .field(field.number())
                .and_then(|raw| field.scaled(&raw.raw_value()));
            match got {
                Some(v) if (v - want.value).abs() <= want.tolerance => checked += 1,
                other => failures.push(format!(
                    "{}: session.{name}: expected {}, got {other:?}",
                    f.name, want.value
                )),
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("checked {checked} session values");
}

/// The hand-picked typed accessors users reach for first.
#[test]
fn typed_session_accessors_match_fitcsvtool() {
    let mut failures = Vec::new();
    for f in load_fixtures() {
        let messages = data_messages(&f.bytes);
        let Some(raw) = nth_message(&messages, Session::GLOBAL, 0) else {
            continue;
        };
        let s = Session::new(raw).unwrap();
        for (name, want) in &f.expected.profile.session {
            let got: Option<f64> = match name.as_str() {
                "start_time" => s.start_time().map(f64::from),
                "sport" => s.sport().map(|v| f64::from(v.to_raw())),
                "sub_sport" => s.sub_sport().map(|v| f64::from(v.to_raw())),
                "total_elapsed_time" => s.total_elapsed_time(),
                "total_timer_time" => s.total_timer_time(),
                "total_distance" => s.total_distance(),
                "total_calories" => s.total_calories().map(f64::from),
                "avg_speed" => s.avg_speed(),
                "max_speed" => s.max_speed(),
                "enhanced_avg_speed" => s.enhanced_avg_speed(),
                "avg_heart_rate" => s.avg_heart_rate().map(f64::from),
                "max_heart_rate" => s.max_heart_rate().map(f64::from),
                "avg_cadence" => s.avg_cadence().map(f64::from),
                "avg_power" => s.avg_power().map(f64::from),
                "max_power" => s.max_power().map(f64::from),
                "normalized_power" => s.normalized_power().map(f64::from),
                "total_ascent" => s.total_ascent().map(f64::from),
                "total_descent" => s.total_descent().map(f64::from),
                "avg_altitude" => s.avg_altitude(),
                "training_stress_score" => s.training_stress_score(),
                _ => continue,
            };
            match got {
                Some(v) if (v - want.value).abs() <= want.tolerance => {}
                other => failures.push(format!(
                    "{}: Session::{name}(): expected {}, got {other:?}",
                    f.name, want.value
                )),
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Spot-checked raw values agree with the typed views' fields: each typed
/// view wraps exactly the message the raw layer decoded.
#[test]
fn every_message_dispatches() {
    for f in load_fixtures() {
        for msg in data_messages(&f.bytes) {
            let typed = Message::new(msg);
            assert_eq!(typed.raw(), &msg, "{}", f.name);
            let covered = message_info(msg.global_message_number()).is_some();
            assert_eq!(covered, !matches!(typed, Message::Other(_)), "{}", f.name);
        }
    }
}

/// Every developer field in a valid file has a `field_description`.
#[cfg(feature = "alloc")]
#[test]
fn developer_fields_resolve() {
    use zerofit::{Decoder, Record};
    use zerofit_profile::developer::DeveloperData;

    let mut resolved = 0;
    for f in load_fixtures() {
        let mut dev = DeveloperData::new();
        for r in Decoder::new(&f.bytes) {
            match r.unwrap() {
                Record::Header(_) => dev.clear(),
                Record::Data(msg) => {
                    dev.observe(&msg);
                    let n = dev.resolve(&msg).count();
                    assert_eq!(n, msg.developer_fields().count(), "{}", f.name);
                    resolved += n;
                }
                _ => {}
            }
        }
    }
    eprintln!("resolved {resolved} developer fields");
}
