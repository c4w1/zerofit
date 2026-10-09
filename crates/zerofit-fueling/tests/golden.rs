//! Golden days: whole-day plans for the look-ahead scenarios, checked
//! end to end (target, band, reason text and the timeline's total).
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use zerofit_fueling::plan::{DayInput, DayPlan, MealSchedule, day_plan};
use zerofit_fueling::{Athlete, DayAhead, LoadBand, PlannedSession, Priority, RaiseReason};

/// The demo athlete.
const ATHLETE: Athlete = Athlete {
    body_mass_kg: 72.0,
    ftp_w: Some(326.0),
};

/// The demo's Saturday: "Long ride with tempo", 4 h from 09:00.
const LONG_RIDE: [PlannedSession; 1] = [PlannedSession::new(9 * 60, 240, 0.70)];

fn plan(today: &[PlannedSession], ahead: &[DayAhead<'_>]) -> DayPlan {
    day_plan(&DayInput {
        athlete: ATHLETE,
        sessions: today,
        ahead,
        schedule: MealSchedule::default(),
    })
    .unwrap()
}

/// The meals add up to the target (rounded to whole grams per entry).
fn adds_up(p: &DayPlan) {
    assert!(
        (p.carbs_planned_g - p.carbs_target_g).abs() <= 2.5,
        "planned {} vs target {}",
        p.carbs_planned_g,
        p.carbs_target_g
    );
    assert!((p.carbs_target_g - p.carbs_g_per_kg * ATHLETE.body_mass_kg).abs() < 1e-9);
    assert_eq!(p.band, LoadBand::for_g_per_kg(p.carbs_g_per_kg));
}

#[test]
fn rest_day_before_a_long_ride_is_raised() {
    let p = plan(
        &[],
        &[DayAhead {
            sessions: &LONG_RIDE,
            priority: None,
        }],
    );
    assert_eq!(p.own_carbs_g_per_kg, 3.0);
    assert_eq!(p.carbs_g_per_kg, 8.0);
    assert_eq!(p.band, LoadBand::High);
    let raise = p.raise.unwrap();
    assert_eq!(
        raise.reason,
        RaiseReason::VeryLongSessionTomorrow { minutes: 240 }
    );
    assert_eq!(raise.from_g_per_kg, 3.0);
    assert_eq!(raise.to_string(), "Raised: 4 h ride tomorrow");
    adds_up(&p);
}

#[test]
fn rest_day_before_a_rest_day_stays_light() {
    let p = plan(&[], &[DayAhead::REST, DayAhead::REST]);
    assert_eq!(p.carbs_g_per_kg, 3.0);
    assert_eq!(p.own_carbs_g_per_kg, 3.0);
    assert_eq!(p.band, LoadBand::Light);
    assert!(p.raise.is_none());
    adds_up(&p);
}

#[test]
fn day_before_an_a_race_is_a_carb_loading_day() {
    // A 3 h A race tomorrow; an easy hour today.
    let race = [PlannedSession::new(8 * 60, 180, 0.85)];
    let today = [PlannedSession::new(10 * 60, 60, 0.55)];
    let p = plan(
        &today,
        &[DayAhead {
            sessions: &race,
            priority: Some(Priority::A),
        }],
    );
    // 10 g/kg at 90 min rising to 12 at 4.5 h: 3 h is halfway.
    assert_eq!(p.carbs_g_per_kg, 11.0);
    assert_eq!(p.band, LoadBand::VeryHigh);
    let raise = p.raise.unwrap();
    assert_eq!(
        raise.reason,
        RaiseReason::CarbLoad {
            days_before: 1,
            event_min: 180
        }
    );
    assert_eq!(raise.to_string(), "Carb-loading: 3 h A event tomorrow");
    adds_up(&p);
}

#[test]
fn two_days_before_an_a_race_is_also_loaded() {
    let race = [PlannedSession::new(8 * 60, 180, 0.85)];
    let p = plan(
        &[],
        &[
            DayAhead::REST,
            DayAhead {
                sessions: &race,
                priority: Some(Priority::A),
            },
        ],
    );
    assert_eq!(p.carbs_g_per_kg, 11.0);
    assert_eq!(
        p.raise.unwrap().to_string(),
        "Carb-loading: 3 h A event in 2 days"
    );
    adds_up(&p);
}

#[test]
fn the_same_race_without_priority_is_a_long_ride() {
    let race = [PlannedSession::new(8 * 60, 180, 0.85)];
    let p = plan(
        &[],
        &[DayAhead {
            sessions: &race,
            priority: Some(Priority::C),
        }],
    );
    // 5 g/kg at 90 min → 7 at 4 h: 3 h is 0.6 of the way.
    assert!((p.carbs_g_per_kg - 6.2).abs() < 1e-9);
    assert_eq!(p.band, LoadBand::Moderate);
    assert_eq!(p.raise.unwrap().to_string(), "Raised: 3 h ride tomorrow");
}

#[test]
fn a_hard_day_is_never_lowered_by_an_easy_tomorrow() {
    // The demo's Thursday (VO2max 5x4, ~75 min) before a rest Friday.
    let vo2 = [PlannedSession::new(18 * 60, 65, 0.80)];
    let p = plan(&vo2, &[DayAhead::REST]);
    assert!(p.raise.is_none());
    assert_eq!(p.carbs_g_per_kg, p.own_carbs_g_per_kg);
    adds_up(&p);
}
