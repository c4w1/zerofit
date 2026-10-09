//! Property tests: more training (today or in the days ahead) never means
//! a lower target, and every plan is internally consistent.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_cmp
)]

use proptest::prelude::*;
use zerofit_fueling::daily::{daily_carbs_g_per_kg, daily_protein_g_per_kg, day_load};
use zerofit_fueling::session::{during_ride, pre_ride, recovery};
use zerofit_fueling::{
    Athlete, DayAhead, DayInput, DayPlan, EntryKind, MealSchedule, PlannedSession, Priority,
    day_plan,
};

/// Sessions laid out from 05:00 with the given gaps, not overlapping.
fn lay_out(sessions: Vec<(u32, u32, f64)>) -> Vec<PlannedSession> {
    let mut t = 300;
    sessions
        .into_iter()
        .map(|(gap, dur, i)| {
            let s = PlannedSession::new(t + gap / 4, dur, i);
            t = s.end_min() + 30;
            s
        })
        .collect()
}

fn plan_with(athlete: Athlete, today: &[PlannedSession], ahead: &[DayAhead<'_>]) -> DayPlan {
    day_plan(&DayInput {
        athlete,
        sessions: today,
        ahead,
        schedule: MealSchedule::default(),
    })
    .unwrap()
}

fn priority() -> impl Strategy<Value = Option<Priority>> {
    prop::option::of(prop_oneof![
        Just(Priority::A),
        Just(Priority::B),
        Just(Priority::C)
    ])
}

fn athlete() -> impl Strategy<Value = Athlete> {
    (40.0f64..120.0, prop::option::of(100.0f64..450.0)).prop_map(|(body_mass_kg, ftp_w)| Athlete {
        body_mass_kg,
        ftp_w,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Daily carbohydrate and protein targets never fall as load rises.
    #[test]
    fn daily_targets_monotone_in_load(a in 0.0f64..100.0, b in 0.0f64..100.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(daily_carbs_g_per_kg(lo) <= daily_carbs_g_per_kg(hi));
        prop_assert!(daily_protein_g_per_kg(lo) <= daily_protein_g_per_kg(hi));
        prop_assert!((3.0..=12.0).contains(&daily_carbs_g_per_kg(a)));
        prop_assert!((1.2..=2.0).contains(&daily_protein_g_per_kg(a)));
    }

    /// Riding longer, harder, or doing more work never lowers the day's
    /// targets, the in-ride rate, the pre-ride meal or the recovery rate.
    #[test]
    fn more_training_never_lowers_any_target(
        athlete in athlete(),
        minutes in 0u32..600, extra_min in 0u32..300,
        intensity in 0.3f64..1.2, extra_if in 0.0f64..0.5,
    ) {
        let base = PlannedSession::new(600, minutes, intensity);
        for more in [
            PlannedSession::new(600, minutes + extra_min, intensity),
            PlannedSession::new(600, minutes, intensity + extra_if),
            PlannedSession::new(600, minutes + extra_min, intensity + extra_if),
        ] {
            let (l0, l1) = (day_load(&[base], &athlete), day_load(&[more], &athlete));
            prop_assert!(l0 <= l1 + 1e-12);
            prop_assert!(daily_carbs_g_per_kg(l0) <= daily_carbs_g_per_kg(l1) + 1e-12);
            prop_assert!(during_ride(&base).carbs_g_per_hour <= during_ride(&more).carbs_g_per_hour);
            let (p0, p1) = (pre_ride(&base, &athlete, 24.0), pre_ride(&more, &athlete, 24.0));
            prop_assert!(p0.carbs_g_per_kg <= p1.carbs_g_per_kg + 1e-12);
            let (r0, r1) = (recovery(&base, &athlete, Some(10.0)), recovery(&more, &athlete, Some(10.0)));
            prop_assert!(r0.carbs_g_per_kg_per_hour <= r1.carbs_g_per_kg_per_hour + 1e-12);
        }
    }

    /// A day's target never drops when the days ahead get harder: a longer
    /// or harder session tomorrow, an extra session, or an event priority.
    #[test]
    fn target_never_drops_when_tomorrow_gets_harder(
        athlete in athlete(),
        today in prop::collection::vec((0u32..720, 1u32..300, 0.3f64..1.1), 0..2),
        start in 300u32..1200, minutes in 1u32..400, intensity in 0.3f64..1.1,
        extra_min in 0u32..200, extra_if in 0.0f64..0.4,
        day_after in prop::option::of((300u32..1200, 1u32..400, 0.3f64..1.1)),
        priority in priority(),
    ) {
        let today = lay_out(today);
        let after = day_after.map(|(s, d, i)| [PlannedSession::new(s, d, i)]);
        let after_day = DayAhead { sessions: after.as_ref().map_or(&[], |a| a), priority: None };
        let base = [PlannedSession::new(start, minutes, intensity)];
        let target = |sessions: &[PlannedSession], p: Option<Priority>| {
            plan_with(athlete, &today, &[DayAhead { sessions, priority: p }, after_day]).carbs_g_per_kg
        };
        let t0 = target(&base, None);
        prop_assert!(target(&[], None) <= t0 + 1e-12);
        prop_assert!(target(&[PlannedSession::new(start, minutes + extra_min, intensity)], None) >= t0 - 1e-12);
        prop_assert!(target(&[PlannedSession::new(start, minutes, intensity + extra_if)], None) >= t0 - 1e-12);
        prop_assert!(target(&[base[0], PlannedSession::new(start + minutes + 30, 60, 0.7)], None) >= t0 - 1e-12);
        // Any priority (A, B, C) is at least as demanding as none.
        prop_assert!(target(&base, priority) >= t0 - 1e-12);
        prop_assert!(target(&base, Some(Priority::A)) >= t0 - 1e-12);
        // And never below the day's own target.
        let own = plan_with(athlete, &today, &[]).carbs_g_per_kg;
        prop_assert!(t0 >= own - 1e-12);
    }

    /// Adding a second session never lowers the daily targets.
    #[test]
    fn adding_a_session_never_lowers_the_day(
        athlete in athlete(), minutes in 1u32..240, intensity in 0.3f64..1.1,
    ) {
        let one = [PlannedSession::new(420, 90, 0.7)];
        let two = [one[0], PlannedSession::new(1000, minutes, intensity)];
        prop_assert!(day_load(&one, &athlete) <= day_load(&two, &athlete));
    }

    /// Every plan: sorted timeline, protein exactly on target, carbohydrate
    /// on target unless the session feeds alone exceed it, nothing
    /// negative, in-ride feeds only during the session.
    #[test]
    fn plans_are_consistent(
        athlete in athlete(),
        sessions in prop::collection::vec((0u32..720, 1u32..360, 0.3f64..1.1), 0..3),
        tomorrow in prop::collection::vec((0u32..720, 1u32..360, 0.3f64..1.1), 0..2),
        tomorrow_priority in priority(),
    ) {
        let sessions = lay_out(sessions);
        let tomorrow = lay_out(tomorrow);
        let plan = plan_with(athlete, &sessions, &[DayAhead { sessions: &tomorrow, priority: tomorrow_priority }]);
        // The label always agrees with the number.
        prop_assert_eq!(plan.band, zerofit_fueling::LoadBand::for_g_per_kg(plan.carbs_g_per_kg));
        prop_assert!(plan.carbs_g_per_kg >= plan.own_carbs_g_per_kg);
        // Every meal meets its floors (within the 5 g rounding); every
        // protein feeding gets an equal share, at least 0.3 g/kg unless
        // the 2.0 g/kg/day cap binds.
        let mass = athlete.body_mass_kg;
        let shares: Vec<f64> = plan.entries.iter().map(|e| e.protein_g).filter(|p| *p > 0.0).collect();
        let feedings = plan.entries.iter().filter(|e| e.kind.meal().is_some()).count()
            + plan.entries.iter().filter(|e| e.kind.meal().is_none() && e.protein_g > 0.0).count();
        for e in &plan.entries {
            if let Some(m) = e.kind.meal() {
                prop_assert!(e.carbs_g >= m.carb_floor_g_per_kg(plan.carbs_g_per_kg) * mass - 5.0, "{:?}", e);
                if plan.protein_g_per_kg < 2.0 - 1e-9 {
                    prop_assert!(e.protein_g >= 0.3 * mass - 5.0, "{:?}", e);
                }
            }
        }
        if let (Some(lo), Some(hi)) = (shares.iter().copied().reduce(f64::min), shares.iter().copied().reduce(f64::max)) {
            prop_assert!(hi - lo <= 5.0 + 1e-9, "uneven protein {:?}", shares);
        }
        prop_assert!(feedings >= 1);
        prop_assert!(plan.entries.windows(2).all(|w| w[0].time_min <= w[1].time_min));
        for e in &plan.entries {
            prop_assert!(e.carbs_g >= 0.0 && e.protein_g >= 0.0);
            prop_assert_eq!(e.time_min % 15, 0, "{:?}", e);
            prop_assert_eq!(e.carbs_g % 5.0, 0.0, "{:?}", e);
            prop_assert_eq!(e.protein_g % 5.0, 0.0, "{:?}", e);
        }
        // Rounding to 5 g moves the total by at most 2.5 g.
        let protein: f64 = plan.entries.iter().map(|e| e.protein_g).sum();
        prop_assert!((protein - plan.protein_g).abs() <= 2.5 + 1e-6);
        if plan.session_feeds_exceed_target {
            prop_assert!(plan.carbs_planned_g >= plan.carbs_target_g - 2.5);
        } else {
            prop_assert!((plan.carbs_planned_g - plan.carbs_target_g).abs() <= 2.5 + 1e-6);
        }
        for e in &plan.entries {
            if let EntryKind::DuringSession { session } = e.kind {
                let s = sessions[session];
                prop_assert!(e.time_min > s.start_min && e.time_min < s.end_min());
            }
        }
    }
}
