//! Fueling around one session: before, during and after.

use crate::daily::session_load;
use crate::{Athlete, PlannedSession};

/// Session load (kJ/kg, see [`session_load`]) at which a session counts
/// as maximally demanding for pre-ride and recovery scaling: ~4 h at a
/// moderate intensity.
pub const FULL_DEMAND_LOAD: f64 = 35.0;

/// How demanding a session is, 0–1: `min(session load / 35, 1)`.
///
/// Used to pick a point inside the pre-ride and recovery ranges.
#[must_use]
pub fn demand(session: &PlannedSession, athlete: &Athlete) -> f64 {
    let load = session_load(session, athlete);
    if load.is_nan() {
        return 0.0;
    }
    (load / FULL_DEMAND_LOAD).clamp(0.0, 1.0)
}

/// The pre-exercise meal.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PreRide {
    /// Hours before the session start, 1–4.
    pub hours_before: f64,
    /// Carbohydrate, g/kg body mass, 1–4.
    pub carbs_g_per_kg: f64,
}

/// The pre-exercise meal: **1–4 g/kg carbohydrate, 1–4 h before**
/// (Thomas, Erdman & Burke 2016; Burke et al. 2011).
///
/// ```text
/// hours = 1 + 3 · demand          (clamped to the time available)
/// g/kg  = hours                   (1 g/kg per hour of lead time)
/// ```
///
/// A short easy ride gets a 1 g/kg snack an hour before; a long hard one
/// gets a 4 g/kg meal 4 h before. Judgment call: tying the amount to the
/// lead time (1 g/kg per hour) is the common practical reading of the
/// consensus — a bigger meal needs longer to empty from the stomach — and
/// keeps an early start from producing an impossible 4 g/kg meal at
/// 05:00. `available_hours` is the time since waking or since the previous
/// session ended; the lead time never exceeds it, and never drops below
/// 1 h.
///
/// ```
/// use zerofit_fueling::{Athlete, PlannedSession, session::pre_ride};
/// let athlete = Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) };
/// let easy = pre_ride(&PlannedSession::new(600, 45, 0.6), &athlete, 10.0);
/// assert!(easy.hours_before < 1.5 && easy.carbs_g_per_kg == easy.hours_before);
/// let long = pre_ride(&PlannedSession::new(600, 300, 0.75), &athlete, 10.0);
/// assert_eq!((long.hours_before, long.carbs_g_per_kg), (4.0, 4.0));
/// let early = pre_ride(&PlannedSession::new(420, 300, 0.75), &athlete, 1.5);
/// assert_eq!(early.hours_before, 1.5);
/// ```
#[must_use]
pub fn pre_ride(session: &PlannedSession, athlete: &Athlete, available_hours: f64) -> PreRide {
    let wanted = 1.0 + 3.0 * demand(session, athlete);
    let available = if available_hours.is_nan() {
        1.0
    } else {
        available_hours.max(1.0)
    };
    let hours = wanted.min(available).clamp(1.0, 4.0);
    PreRide {
        hours_before: hours,
        carbs_g_per_kg: hours,
    }
}

/// Carbohydrate during the session.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DuringRide {
    /// Carbohydrate per hour, g.
    pub carbs_g_per_hour: f64,
    /// Whether the rate needs multiple transportable carbohydrates
    /// (glucose + fructose): above 60 g/h a single sugar's intestinal
    /// transporter (SGLT1) saturates.
    pub multiple_transportable: bool,
    /// Whether a carbohydrate mouth rinse is worth suggesting: a
    /// 45–75 min session at a high intensity, where rinsing improves
    /// performance even without much carbohydrate absorbed (Carter et al.
    /// 2004; Jeukendrup 2014).
    pub mouth_rinse: bool,
    /// Minutes between feeds (0 if no feeding is needed).
    pub feed_interval_min: u32,
}

/// Minutes between in-ride feeds.
pub const FEED_INTERVAL_MIN: u32 = 30;

/// Intensity factor from which a 45–75 min session gets the mouth-rinse
/// suggestion: threshold-type work and above.
pub const MOUTH_RINSE_IF: f64 = 0.85;

/// In-ride carbohydrate by duration, adjusted for intensity (Jeukendrup
/// 2014, *Sports Med* 44:S25, Figure 2; Thomas, Erdman & Burke 2016):
///
/// | duration | g/h | at IF ≤ 0.55 → IF ≥ 0.85 |
/// |---|---|---|
/// | < 45 min | none | 0 |
/// | 45–75 min | small amounts or a mouth rinse | 0 → 30 |
/// | 75 min–2 h | 30–60 | 30 → 60 |
/// | 2–2.5 h | up to 60 | 45 → 60 |
/// | > 2.5 h | 60–90, glucose + fructose above 60 | 60 → 90 |
///
/// Inside each band intensity picks the point:
/// `s = clamp((IF − 0.55) / 0.30, 0, 1)`, rounded to whole grams. The 2–3 h
/// and "> 2.5 h" rows of the paper overlap; 150 min is the boundary here.
/// Each band's floor is at least the previous band's ceiling at the same
/// intensity, so the rate never decreases with duration or intensity (a
/// property test). Boundaries belong to the lower band: exactly 45 min
/// gets 0, 75 min is in the 0–30 band, 150 min in the 45–60 band.
///
/// ```
/// use zerofit_fueling::{PlannedSession, session::during_ride};
/// let rate = |min, i| during_ride(&PlannedSession::new(0, min, i)).carbs_g_per_hour;
/// assert_eq!(rate(44, 1.0), 0.0);
/// assert_eq!(rate(65, 0.55), 0.0);
/// assert_eq!(rate(65, 0.85), 30.0);
/// assert_eq!(rate(90, 0.70), 45.0);
/// assert_eq!(rate(150, 0.55), 45.0);
/// let long = during_ride(&PlannedSession::new(0, 240, 0.85));
/// assert_eq!(long.carbs_g_per_hour, 90.0);
/// assert!(long.multiple_transportable);
/// assert!(during_ride(&PlannedSession::new(0, 60, 0.9)).mouth_rinse);
/// ```
#[must_use]
pub fn during_ride(session: &PlannedSession) -> DuringRide {
    let intensity = session.intensity_factor;
    let s = if intensity.is_nan() {
        0.0
    } else {
        ((intensity - 0.55) / 0.30).clamp(0.0, 1.0)
    };
    let (exact, rinse_band) = match session.duration_min {
        0..=45 => (0.0, false),
        46..=75 => (30.0 * s, true),
        76..=120 => (30.0 + 30.0 * s, false),
        121..=150 => (45.0 + 15.0 * s, false),
        _ => (60.0 + 30.0 * s, false),
    };
    // Whole grams per hour: the precision anyone can eat to, and it keeps
    // IF 0.85 at exactly the top of its range despite float rounding.
    let rate = f64::from(crate::num::round_u32(exact));
    DuringRide {
        carbs_g_per_hour: rate,
        multiple_transportable: rate > 60.0,
        mouth_rinse: rinse_band && intensity >= MOUTH_RINSE_IF,
        feed_interval_min: if rate > 0.0 { FEED_INTERVAL_MIN } else { 0 },
    }
}

/// Post-exercise recovery.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Recovery {
    /// Carbohydrate per hour, g/kg/h (0 when recovery is not urgent).
    pub carbs_g_per_kg_per_hour: f64,
    /// Hours of hourly recovery feeds, 0–4.
    pub hours: u32,
    /// Protein with the first feed, g/kg.
    pub protein_g_per_kg: f64,
}

/// Hours between sessions below which recovery is "speedy refuelling".
///
/// Burke et al. 2011 (IOC consensus) and Thomas, Erdman & Burke 2016:
/// *"when the period between exercise sessions is < 8 h"*. With a longer
/// gap, regular meals that meet the daily target restore glycogen just
/// as well (Burke et al. 1996; Parkin et al. 1997). An earlier version of
/// this crate used 24 h, which put four hourly feeds after every evening
/// session followed by a morning ride and starved the rest of the day.
pub const RAPID_RECOVERY_HOURS: f64 = 8.0;

/// Recovery fueling (Burke et al. 2011; Thomas, Erdman & Burke 2016):
/// when the next session is **less than [`RAPID_RECOVERY_HOURS`]** after
/// this one ends, eat **1.0–1.2 g/kg/h carbohydrate for the first ~4 h**,
/// starting soon after the session, to restore muscle glycogen at the
/// fastest rate. With more time, regular meals restore glycogen and no
/// rapid recovery is needed.
///
/// The rate is `1.0 + 0.2 · demand`. The hours are `min(4, hours until the
/// next session)`, rounded down. Either way the first feed carries
/// ~0.3 g/kg protein (Moore et al. 2009, 2015), which supports muscle
/// repair and adds to glycogen resynthesis when carbohydrate is below
/// ~1.2 g/kg/h.
///
/// ```
/// use zerofit_fueling::{Athlete, PlannedSession, session::recovery};
/// let athlete = Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) };
/// let ride = PlannedSession::new(480, 180, 0.75);
/// let double_day = recovery(&ride, &athlete, Some(6.0));
/// assert_eq!(double_day.hours, 4);
/// assert!(double_day.carbs_g_per_kg_per_hour >= 1.0);
/// let rest_tomorrow = recovery(&ride, &athlete, Some(20.0));
/// assert_eq!((rest_tomorrow.hours, rest_tomorrow.carbs_g_per_kg_per_hour), (0, 0.0));
/// assert_eq!(rest_tomorrow.protein_g_per_kg, 0.3);
/// ```
#[must_use]
pub fn recovery(
    session: &PlannedSession,
    athlete: &Athlete,
    hours_until_next: Option<f64>,
) -> Recovery {
    let urgent = hours_until_next.filter(|h| h.is_finite() && *h < RAPID_RECOVERY_HOURS);
    match urgent {
        Some(gap) => {
            let hours = crate::num::floor_u32(gap.clamp(0.0, 4.0)).max(1);
            Recovery {
                carbs_g_per_kg_per_hour: 1.0 + 0.2 * demand(session, athlete),
                hours,
                protein_g_per_kg: RECOVERY_PROTEIN_G_PER_KG,
            }
        }
        None => Recovery {
            carbs_g_per_kg_per_hour: 0.0,
            hours: 0,
            protein_g_per_kg: RECOVERY_PROTEIN_G_PER_KG,
        },
    }
}

/// Protein per feeding, g/kg: ~0.3 g/kg maximally stimulates muscle
/// protein synthesis (Moore et al. 2015).
pub const RECOVERY_PROTEIN_G_PER_KG: f64 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;

    const A: Athlete = Athlete {
        body_mass_kg: 70.0,
        ftp_w: Some(250.0),
    };

    #[test]
    fn during_ride_boundaries() {
        let rate = |min, i| during_ride(&PlannedSession::new(0, min, i)).carbs_g_per_hour;
        assert_eq!(rate(0, 1.0), 0.0);
        assert_eq!(rate(45, 1.0), 0.0);
        assert_eq!(rate(46, 0.5), 0.0);
        assert_eq!(rate(46, 1.0), 30.0);
        assert_eq!(rate(75, 0.70), 15.0);
        assert_eq!(rate(76, 0.5), 30.0);
        assert_eq!(rate(120, 1.0), 60.0);
        assert_eq!(rate(121, 0.5), 45.0);
        assert_eq!(rate(150, 1.0), 60.0);
        assert_eq!(rate(151, 0.5), 60.0);
        assert_eq!(rate(151, 1.0), 90.0);
        assert_eq!(rate(600, 0.70), 75.0);
        assert_eq!(rate(90, f64::NAN), 30.0);
        assert!(!during_ride(&PlannedSession::new(0, 90, 1.0)).multiple_transportable);
        assert_eq!(
            during_ride(&PlannedSession::new(0, 30, 1.0)).feed_interval_min,
            0
        );
        // The demo's 65-min VO2 session: well under the old 60 g/h.
        assert!(rate(65, 0.80) <= 30.0);
        assert!(!during_ride(&PlannedSession::new(0, 65, 0.7)).mouth_rinse);
        assert!(!during_ride(&PlannedSession::new(0, 90, 0.95)).mouth_rinse);
    }

    #[test]
    fn during_ride_is_monotone() {
        let mut last_by_if = [0.0f64; 7];
        for minutes in 0..400 {
            let mut last = 0.0;
            for (k, slot) in last_by_if.iter_mut().enumerate() {
                let intensity = 0.5 + 0.08 * f64::from(u8::try_from(k).unwrap());
                let r = during_ride(&PlannedSession::new(0, minutes, intensity)).carbs_g_per_hour;
                assert!(r >= last, "{minutes} min IF {intensity}");
                assert!(r >= *slot, "{minutes} min IF {intensity}");
                last = r;
                *slot = r;
            }
        }
    }

    #[test]
    fn pre_ride_ranges() {
        for minutes in [10, 60, 120, 240, 600] {
            for intensity in [0.4, 0.7, 1.0] {
                let p = pre_ride(&PlannedSession::new(600, minutes, intensity), &A, 24.0);
                assert!((1.0..=4.0).contains(&p.hours_before));
                assert!((1.0..=4.0).contains(&p.carbs_g_per_kg));
            }
        }
        // Less than an hour available still gives a 1 h / 1 g/kg snack.
        let p = pre_ride(&PlannedSession::new(400, 300, 0.8), &A, 0.25);
        assert_eq!((p.hours_before, p.carbs_g_per_kg), (1.0, 1.0));
        let p = pre_ride(&PlannedSession::new(400, 300, 0.8), &A, f64::NAN);
        assert_eq!(p.hours_before, 1.0);
    }

    #[test]
    fn recovery_window() {
        let ride = PlannedSession::new(480, 90, 0.7);
        assert_eq!(recovery(&ride, &A, Some(7.99)).hours, 4);
        assert_eq!(recovery(&ride, &A, Some(8.0)).hours, 0);
        assert_eq!(recovery(&ride, &A, Some(20.0)).hours, 0);
        assert_eq!(recovery(&ride, &A, None).hours, 0);
        assert_eq!(recovery(&ride, &A, Some(2.5)).hours, 2);
        assert_eq!(recovery(&ride, &A, Some(0.2)).hours, 1);
        let r = recovery(&ride, &A, Some(5.0)).carbs_g_per_kg_per_hour;
        assert!((1.0..=1.2).contains(&r));
        let max = recovery(&PlannedSession::new(0, 600, 1.0), &A, Some(5.0));
        assert!((max.carbs_g_per_kg_per_hour - 1.2).abs() < 1e-12);
    }

    #[test]
    fn demand_is_bounded() {
        assert_eq!(demand(&PlannedSession::new(0, 0, 0.8), &A), 0.0);
        assert_eq!(demand(&PlannedSession::new(0, 6000, 0.8), &A), 1.0);
    }
}
