//! Assembling a day plan: totals, per-meal targets and a timeline around
//! each session.

use alloc::vec::Vec;

use crate::daily::{
    LoadBand, daily_carbs_g_per_kg, daily_protein_g_per_kg, day_load, session_load,
};
use crate::num::{f64_from_usize, floor_u32, round_u32};
use crate::session::{DuringRide, PreRide, Recovery, during_ride, pre_ride, recovery};
use crate::{Athlete, PlannedSession};

/// A regular meal of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MealKind {
    /// Breakfast.
    Breakfast,
    /// Lunch.
    Lunch,
    /// Dinner.
    Dinner,
    /// Evening snack (protein-focused, half a meal's carbohydrate share).
    EveningSnack,
}

impl MealKind {
    /// Share of the remaining carbohydrate relative to a main meal.
    const fn carb_weight(self) -> f64 {
        match self {
            Self::EveningSnack => 0.5,
            _ => 1.0,
        }
    }
}

/// When the athlete wakes and normally eats, minutes after midnight.
///
/// ```
/// use zerofit_fueling::plan::MealSchedule;
/// let s = MealSchedule::default();
/// assert_eq!(s.wake_min, 6 * 60 + 30);
/// assert_eq!(s.meals.len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MealSchedule {
    /// Wake time; the earliest a pre-session meal can be.
    pub wake_min: u32,
    /// Regular meals: (time, kind).
    pub meals: Vec<(u32, MealKind)>,
}

impl Default for MealSchedule {
    fn default() -> Self {
        Self {
            wake_min: 390,
            meals: alloc::vec![
                (420, MealKind::Breakfast),
                (750, MealKind::Lunch),
                (1110, MealKind::Dinner),
                (1290, MealKind::EveningSnack),
            ],
        }
    }
}

/// What an entry in the day timeline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "snake_case"))]
pub enum EntryKind {
    /// A regular meal.
    Meal {
        /// Which one.
        meal: MealKind,
    },
    /// The pre-session meal or snack.
    PreSession {
        /// Index of the session in the day.
        session: usize,
    },
    /// A feed during the session.
    DuringSession {
        /// Index of the session in the day.
        session: usize,
    },
    /// A recovery feed after the session.
    Recovery {
        /// Index of the session in the day.
        session: usize,
        /// Hours after the session ended (0 = immediately).
        hour: u32,
    },
}

/// One item of the day timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Entry {
    /// Minutes after midnight (may exceed 1440 for recovery after a late
    /// session).
    pub time_min: u32,
    /// What it is.
    pub kind: EntryKind,
    /// Carbohydrate, g.
    pub carbs_g: f64,
    /// Protein, g.
    pub protein_g: f64,
}

/// The plan for one session.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SessionPlan {
    /// Start, minutes after midnight.
    pub start_min: u32,
    /// Duration, minutes.
    pub duration_min: u32,
    /// Effective load, kJ/kg.
    pub load_kj_per_kg: f64,
    /// Pre-session meal.
    pub pre: PreRide,
    /// In-session carbohydrate.
    pub during: DuringRide,
    /// In-session carbohydrate in total, g.
    pub during_total_g: f64,
    /// Recovery after the session.
    pub recovery: Recovery,
}

/// A full day: targets, sessions and the timeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct DayPlan {
    /// Effective day load, kJ/kg.
    pub load_kj_per_kg: f64,
    /// Load category.
    pub band: LoadBand,
    /// Carbohydrate target, g/kg.
    pub carbs_g_per_kg: f64,
    /// Carbohydrate target, g.
    pub carbs_target_g: f64,
    /// Carbohydrate in the timeline, g. Equals the target unless the
    /// session feeds alone exceed it (`session_feeds_exceed_target`).
    pub carbs_planned_g: f64,
    /// Protein target, g/kg.
    pub protein_g_per_kg: f64,
    /// Protein target, g (all of it is in the timeline).
    pub protein_g: f64,
    /// Whether pre/during/recovery feeds alone exceed the daily
    /// carbohydrate target (very long or back-to-back sessions, or a first
    /// recovery hour that doesn't fit the budget); the session rules then
    /// take precedence and the regular meals get no carbohydrate.
    pub session_feeds_exceed_target: bool,
    /// Per-session plans, in start order.
    pub sessions: Vec<SessionPlan>,
    /// Everything to eat, in time order.
    pub entries: Vec<Entry>,
}

/// Why a plan could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    /// Body mass is not a positive, finite number.
    InvalidBodyMass,
    /// Session `index` starts before the previous one ends (sessions must
    /// be sorted by start time and not overlap).
    Overlap {
        /// Index of the offending session.
        index: usize,
    },
}

impl core::fmt::Display for PlanError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidBodyMass => write!(f, "body mass must be a positive number"),
            Self::Overlap { index } => {
                write!(f, "session {index} starts before the previous session ends")
            }
        }
    }
}

impl core::error::Error for PlanError {}

/// The input for [`day_plan`].
#[derive(Debug, Clone, PartialEq)]
pub struct DayInput<'a> {
    /// The athlete.
    pub athlete: Athlete,
    /// The day's sessions, sorted by start, not overlapping.
    pub sessions: &'a [PlannedSession],
    /// Start of the first session after this day, minutes after *this*
    /// day's midnight (e.g. 1440 + 420 for 07:00 tomorrow); decides
    /// whether the last session needs rapid recovery. `None` if none is
    /// planned.
    pub next_session_start_min: Option<u32>,
    /// Wake time and regular meals.
    pub schedule: MealSchedule,
}

/// Minutes around a session-related feed within which a regular meal is
/// dropped (the session feed replaces it).
const MEAL_MERGE_WINDOW_MIN: u32 = 60;

/// Builds the day plan.
///
/// 1. Targets: [`day_load`] → [`daily_carbs_g_per_kg`] and
///    [`daily_protein_g_per_kg`], times body mass.
/// 2. For each session: the pre-session meal ([`pre_ride`]) no earlier
///    than waking or the previous session's end, and in-session feeds
///    every 20 min ([`during_ride`]).
/// 3. Recovery ([`recovery`]) when the next session is under 24 h away:
///    hourly feeds at 1.0–1.2 g/kg/h from the session end, for up to 4 h.
///    The consensus targets are *daily totals*, and session feeds are
///    timing within them (Burke et al. 2011), so recovery hours come out
///    of the day's remaining carbohydrate budget: as many hours as the
///    budget allows, at least one, and never past the next session's
///    pre-meal (which takes over). A day whose session feeds exceed the
///    target keeps the session rules and is flagged
///    (`session_feeds_exceed_target`).
/// 4. Regular meals within an hour of a session feed, or during a
///    session, are dropped: the session feed takes their place.
/// 5. The carbohydrate left is spread over the remaining meals (the
///    evening snack gets half a share); the protein left after session
///    feeds is spread evenly, which puts roughly the recommended
///    0.3 g/kg into each meal.
///
/// # Errors
///
/// [`PlanError`] for a non-positive body mass or overlapping sessions.
///
/// ```
/// use zerofit_fueling::{Athlete, PlannedSession, plan::{DayInput, MealSchedule, day_plan}};
/// let sessions = [PlannedSession::new(9 * 60, 180, 0.75)];
/// let plan = day_plan(&DayInput {
///     athlete: Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) },
///     sessions: &sessions,
///     next_session_start_min: Some(24 * 60 + 9 * 60), // same time tomorrow
///     schedule: MealSchedule::default(),
/// })?;
/// assert!((plan.carbs_planned_g - plan.carbs_target_g).abs() < 1e-6);
/// assert!(plan.sessions[0].during.carbs_g_per_hour >= 60.0); // a 3 h ride
/// assert!(plan.sessions[0].recovery.hours >= 1); // next ride within 24 h
/// assert!(plan.entries.windows(2).all(|w| w[0].time_min <= w[1].time_min));
/// # Ok::<(), zerofit_fueling::plan::PlanError>(())
/// ```
pub fn day_plan(input: &DayInput<'_>) -> Result<DayPlan, PlanError> {
    let athlete = input.athlete;
    let mass = athlete.body_mass_kg;
    validate(input)?;

    let load = day_load(input.sessions, &athlete);
    let carbs_g_per_kg = daily_carbs_g_per_kg(load);
    let protein_g_per_kg = daily_protein_g_per_kg(load);
    let carbs_target_g = carbs_g_per_kg * mass;
    let protein_g = protein_g_per_kg * mass;

    let mut entries: Vec<Entry> = Vec::new();
    let mut sessions = session_feeds(input, &mut entries);
    let session_carbs: f64 = entries.iter().map(|e| e.carbs_g).sum();
    let budget = carbs_target_g - session_carbs;
    recovery_feeds(input, &mut sessions, &mut entries, budget);
    // After recovery: the first recovery hour is kept even when the budget
    // is spent, so it can be what tips the day over.
    let all_session_carbs: f64 = entries.iter().map(|e| e.carbs_g).sum();
    let session_feeds_exceed_target = all_session_carbs > carbs_target_g + 1e-9;

    let kept = kept_meals(input, &entries);
    let carbs_left = (carbs_target_g - entries.iter().map(|e| e.carbs_g).sum::<f64>()).max(0.0);
    let protein_left = (protein_g - entries.iter().map(|e| e.protein_g).sum::<f64>()).max(0.0);
    push_meals(&mut entries, &kept, carbs_left, protein_left);

    entries.sort_by_key(|e| e.time_min);
    let carbs_planned_g = entries.iter().map(|e| e.carbs_g).sum();
    Ok(DayPlan {
        load_kj_per_kg: load,
        band: LoadBand::for_load(load),
        carbs_g_per_kg,
        carbs_target_g,
        carbs_planned_g,
        protein_g_per_kg,
        protein_g,
        session_feeds_exceed_target,
        sessions,
        entries,
    })
}

fn validate(input: &DayInput<'_>) -> Result<(), PlanError> {
    let mass = input.athlete.body_mass_kg;
    if !(mass.is_finite() && mass > 0.0) {
        return Err(PlanError::InvalidBodyMass);
    }
    for (index, pair) in input.sessions.windows(2).enumerate() {
        if let [a, b] = pair {
            if b.start_min < a.end_min() {
                return Err(PlanError::Overlap {
                    index: index.saturating_add(1),
                });
            }
        }
    }
    Ok(())
}

/// Pre-session meals and in-session feeds for every session; returns the
/// session plans with recovery not yet decided.
fn session_feeds(input: &DayInput<'_>, entries: &mut Vec<Entry>) -> Vec<SessionPlan> {
    let athlete = input.athlete;
    let mass = athlete.body_mass_kg;
    let mut sessions = Vec::with_capacity(input.sessions.len());
    let mut earliest = input.schedule.wake_min;
    for (index, s) in input.sessions.iter().enumerate() {
        let available_h = f64::from(s.start_min.saturating_sub(earliest)) / 60.0;
        let pre = pre_ride(s, &athlete, available_h);
        let lead_min = round_u32(pre.hours_before * 60.0);
        entries.push(Entry {
            time_min: s.start_min.saturating_sub(lead_min),
            kind: EntryKind::PreSession { session: index },
            carbs_g: pre.carbs_g_per_kg * mass,
            // A meal two or more hours out is a real meal with protein; a
            // snack closer to the start stays carbohydrate-only.
            protein_g: if pre.hours_before >= 2.0 {
                0.3 * mass
            } else {
                0.0
            },
        });
        let during = during_ride(s);
        let during_total_g = during.carbs_g_per_hour * f64::from(s.duration_min) / 60.0;
        push_during_feeds(entries, index, s, &during, during_total_g);
        sessions.push(SessionPlan {
            start_min: s.start_min,
            duration_min: s.duration_min,
            load_kj_per_kg: session_load(s, &athlete),
            pre,
            during,
            during_total_g,
            recovery: Recovery {
                carbs_g_per_kg_per_hour: 0.0,
                hours: 0,
                protein_g_per_kg: 0.0,
            },
        });
        earliest = s.end_min();
    }
    sessions
}

/// Recovery for each session (see [`day_plan`], step 3), spending at most
/// `budget` grams of carbohydrate.
fn recovery_feeds(
    input: &DayInput<'_>,
    sessions: &mut [SessionPlan],
    entries: &mut Vec<Entry>,
    mut budget: f64,
) {
    let athlete = input.athlete;
    let mass = athlete.body_mass_kg;
    let pre_time = |i: usize| {
        entries
            .iter()
            .find(|e| e.kind == EntryKind::PreSession { session: i })
            .map(|e| e.time_min)
    };
    let mut new_entries = Vec::new();
    for (index, (s, plan)) in input.sessions.iter().zip(sessions.iter_mut()).enumerate() {
        let next = index.saturating_add(1);
        let next_start = input
            .sessions
            .get(next)
            .map(|n| n.start_min)
            .or(input.next_session_start_min);
        let hours_until_next = next_start.map(|n| f64::from(n.saturating_sub(s.end_min())) / 60.0);
        let mut rec = recovery(s, &athlete, hours_until_next);
        let per_hour_g = rec.carbs_g_per_kg_per_hour * mass;
        if rec.hours > 0 && per_hour_g > 0.0 {
            let affordable = floor_u32(budget.max(0.0) / per_hour_g).max(1);
            let next_pre = pre_time(next);
            let before_next_pre = (0..rec.hours)
                .take_while(|h| {
                    let t = s.end_min().saturating_add(h.saturating_mul(60));
                    next_pre.is_none_or(|p| t < p)
                })
                .count();
            let hours = rec
                .hours
                .min(affordable)
                .min(u32::try_from(before_next_pre).unwrap_or(u32::MAX));
            rec.hours = hours;
            budget -= per_hour_g * f64::from(hours);
        }
        push_recovery_feeds(&mut new_entries, index, s, &rec, mass);
        plan.recovery = rec;
    }
    entries.extend(new_entries);
}

/// Regular meals not displaced by a session feed or the session itself.
fn kept_meals(input: &DayInput<'_>, entries: &[Entry]) -> Vec<(u32, MealKind)> {
    input
        .schedule
        .meals
        .iter()
        .copied()
        .filter(|&(t, _)| {
            let near_feed = entries.iter().any(|e| {
                !matches!(e.kind, EntryKind::DuringSession { .. })
                    && t.abs_diff(e.time_min) < MEAL_MERGE_WINDOW_MIN
            });
            let in_session = input
                .sessions
                .iter()
                .any(|s| t >= s.start_min.saturating_sub(30) && t < s.end_min());
            !near_feed && !in_session
        })
        .collect()
}

/// Spreads the remaining carbohydrate (weighted) and protein (evenly)
/// over the kept meals, or into one evening meal if none are left.
fn push_meals(entries: &mut Vec<Entry>, kept: &[(u32, MealKind)], carbs: f64, protein: f64) {
    if kept.is_empty() {
        if carbs > 0.0 || protein > 0.0 {
            let last = entries.iter().map(|e| e.time_min).max().unwrap_or(0);
            entries.push(Entry {
                time_min: last.saturating_add(MEAL_MERGE_WINDOW_MIN).max(1110),
                kind: EntryKind::Meal {
                    meal: MealKind::Dinner,
                },
                carbs_g: carbs,
                protein_g: protein,
            });
        }
        return;
    }
    let weights: f64 = kept.iter().map(|(_, m)| m.carb_weight()).sum();
    let n = f64_from_usize(kept.len());
    for &(t, meal) in kept {
        entries.push(Entry {
            time_min: t,
            kind: EntryKind::Meal { meal },
            carbs_g: carbs * meal.carb_weight() / weights,
            protein_g: protein / n,
        });
    }
}

/// Feeds every `feed_interval_min` from the first interval until before
/// the end, sharing `total_g` equally.
fn push_during_feeds(
    entries: &mut Vec<Entry>,
    index: usize,
    s: &PlannedSession,
    during: &DuringRide,
    total_g: f64,
) {
    let Some(count) = s
        .duration_min
        .saturating_sub(1)
        .checked_div(during.feed_interval_min)
    else {
        return;
    };
    if count == 0 || total_g <= 0.0 {
        return;
    }
    let per_feed = total_g / f64::from(count);
    for k in 1..=count {
        entries.push(Entry {
            time_min: s
                .start_min
                .saturating_add(k.saturating_mul(during.feed_interval_min)),
            kind: EntryKind::DuringSession { session: index },
            carbs_g: per_feed,
            protein_g: 0.0,
        });
    }
}

/// Hourly recovery feeds from the session end; or, without rapid
/// recovery, one protein feed at the end.
fn push_recovery_feeds(
    entries: &mut Vec<Entry>,
    index: usize,
    s: &PlannedSession,
    rec: &Recovery,
    mass: f64,
) {
    let end = s.end_min();
    let protein = rec.protein_g_per_kg * mass;
    if rec.hours == 0 {
        entries.push(Entry {
            time_min: end,
            kind: EntryKind::Recovery {
                session: index,
                hour: 0,
            },
            carbs_g: 0.0,
            protein_g: protein,
        });
        return;
    }
    for hour in 0..rec.hours {
        entries.push(Entry {
            time_min: end.saturating_add(hour.saturating_mul(60)),
            kind: EntryKind::Recovery {
                session: index,
                hour,
            },
            carbs_g: rec.carbs_g_per_kg_per_hour * mass,
            protein_g: if hour == 0 { protein } else { 0.0 },
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const A: Athlete = Athlete {
        body_mass_kg: 70.0,
        ftp_w: Some(250.0),
    };

    fn plan(sessions: &[PlannedSession], next: Option<u32>) -> DayPlan {
        day_plan(&DayInput {
            athlete: A,
            sessions,
            next_session_start_min: next,
            schedule: MealSchedule::default(),
        })
        .unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn rest_day_is_meals_only() {
        let p = plan(&[], None);
        assert_eq!(p.band, LoadBand::Light);
        assert!(close(p.carbs_target_g, 210.0)); // 3 g/kg
        assert!(close(p.protein_g, 84.0)); // 1.2 g/kg
        assert_eq!(p.entries.len(), 4);
        assert!(close(p.carbs_planned_g, 210.0));
        // Breakfast/lunch/dinner get 60 g, the snack 30 g.
        assert!(close(p.entries[0].carbs_g, 60.0));
        assert!(close(p.entries[3].carbs_g, 30.0));
        assert!(p.entries.iter().all(|e| close(e.protein_g, 21.0)));
    }

    #[test]
    fn long_ride_timeline() {
        // 09:00, 3 h at IF 0.75; a ride tomorrow at 09:00 (21 h later).
        let p = plan(&[PlannedSession::new(540, 180, 0.75)], Some(1440 + 540));
        let s = p.sessions[0];
        assert!(s.during.carbs_g_per_hour >= 60.0);
        let during: Vec<_> = p
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::DuringSession { .. }))
            .collect();
        assert_eq!(during.len(), 8); // 20, 40, …, 160 min
        let during_sum: f64 = during.iter().map(|e| e.carbs_g).sum();
        assert!(close(during_sum, s.during_total_g));
        // Pre (2.5 g/kg) + in-ride (80 g/h) leave 162 g of the 577 g
        // target: one hour of recovery at 1.17 g/kg/h fits.
        let recovery = p
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::Recovery { .. }))
            .count();
        assert_eq!(recovery, 1);
        assert_eq!(s.recovery.hours, 1);
        assert!(!p.session_feeds_exceed_target);
        // Breakfast at 07:00 is within an hour of the pre-meal (≥ 06:30
        // given the 06:30 wake) and is replaced; lunch at 12:30 falls in
        // the ride window and is replaced too.
        assert!(!p.entries.iter().any(|e| matches!(
            e.kind,
            EntryKind::Meal {
                meal: MealKind::Breakfast | MealKind::Lunch
            }
        )));
        assert!(close(p.carbs_planned_g, p.carbs_target_g));
        assert!(close(
            p.entries.iter().map(|e| e.protein_g).sum::<f64>(),
            p.protein_g
        ));
    }

    #[test]
    fn no_rapid_recovery_when_next_session_is_far() {
        let p = plan(&[PlannedSession::new(600, 90, 0.7)], None);
        let rec: Vec<_> = p
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::Recovery { .. }))
            .collect();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].carbs_g, 0.0);
        assert!(close(rec[0].protein_g, 21.0));
    }

    #[test]
    fn double_day_recovery_stops_at_next_pre_meal() {
        // 07:00 2 h, then 15:00 1 h.
        let sessions = [
            PlannedSession::new(420, 120, 0.8),
            PlannedSession::new(900, 60, 0.9),
        ];
        let p = plan(&sessions, None);
        let pre2 = p
            .entries
            .iter()
            .find(|e| e.kind == EntryKind::PreSession { session: 1 })
            .unwrap()
            .time_min;
        assert!(p.entries.iter().all(|e| match e.kind {
            EntryKind::Recovery { session: 0, .. } => e.time_min < pre2,
            _ => true,
        }));
        // First session's pre-meal can't be before waking: ≤ 30 min lead
        // available, so 1 h / 1 g/kg.
        assert!(close(p.sessions[0].pre.hours_before, 1.0));
    }

    #[test]
    fn feeds_exceeding_target_are_flagged() {
        // 7 h hard ride and another long ride tomorrow morning.
        let p = plan(&[PlannedSession::new(480, 420, 0.95)], Some(1440 + 420));
        assert_eq!(p.band, LoadBand::VeryHigh);
        if p.session_feeds_exceed_target {
            assert!(p.carbs_planned_g >= p.carbs_target_g);
        } else {
            assert!(close(p.carbs_planned_g, p.carbs_target_g));
        }
    }

    #[test]
    fn errors() {
        let input = |mass: f64, sessions| DayInput {
            athlete: Athlete {
                body_mass_kg: mass,
                ftp_w: None,
            },
            sessions,
            next_session_start_min: None,
            schedule: MealSchedule::default(),
        };
        assert_eq!(day_plan(&input(0.0, &[])), Err(PlanError::InvalidBodyMass));
        assert_eq!(
            day_plan(&input(f64::NAN, &[])),
            Err(PlanError::InvalidBodyMass)
        );
        let overlapping = [
            PlannedSession::new(600, 120, 0.7),
            PlannedSession::new(660, 60, 0.7),
        ];
        assert_eq!(
            day_plan(&input(70.0, &overlapping)),
            Err(PlanError::Overlap { index: 1 })
        );
        let unsorted = [
            PlannedSession::new(900, 60, 0.7),
            PlannedSession::new(600, 60, 0.7),
        ];
        assert!(day_plan(&input(70.0, &unsorted)).is_err());
    }

    #[test]
    fn all_meals_displaced_gets_an_evening_meal() {
        let schedule = MealSchedule {
            wake_min: 390,
            meals: vec![(600, MealKind::Lunch)],
        };
        let sessions = [PlannedSession::new(540, 120, 0.7)];
        let p = day_plan(&DayInput {
            athlete: A,
            sessions: &sessions,
            next_session_start_min: None,
            schedule,
        })
        .unwrap();
        assert!(p.entries.iter().any(|e| e.kind
            == EntryKind::Meal {
                meal: MealKind::Dinner
            }));
        assert!(close(p.carbs_planned_g, p.carbs_target_g));
    }
}
