//! Property tests: more training never means a lower target, and every
//! plan is internally consistent.
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
use zerofit_fueling::{Athlete, DayInput, EntryKind, MealSchedule, PlannedSession, day_plan};

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
        next in prop::option::of(1440u32..3000),
    ) {
        // Lay the sessions out back to back from 05:00 with gaps.
        let mut t = 300;
        let sessions: Vec<PlannedSession> = sessions
            .into_iter()
            .map(|(gap, dur, i)| {
                let s = PlannedSession::new(t + gap / 4, dur, i);
                t = s.end_min() + 30;
                s
            })
            .collect();
        let plan = day_plan(&DayInput {
            athlete,
            sessions: &sessions,
            next_session_start_min: next.map(|n| n.max(t)),
            schedule: MealSchedule::default(),
        })
        .unwrap();
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
