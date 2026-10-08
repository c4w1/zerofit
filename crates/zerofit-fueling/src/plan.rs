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
    /// Dinner moved to after an evening session; it also serves as the
    /// recovery meal.
    PostRideDinner,
    /// Evening snack (protein-focused, half a meal's carbohydrate share).
    EveningSnack,
    /// A protein snack added in a long gap between feedings, so that no
    /// feeding needs more than [`MAX_PROTEIN_PER_FEEDING_G_PER_KG`]
    /// (half a meal's carbohydrate share).
    Snack,
}

impl MealKind {
    /// Share of the remaining carbohydrate relative to a main meal.
    const fn carb_weight(self) -> f64 {
        if self.is_snack() { 0.5 } else { 1.0 }
    }

    /// Whether this is a snack rather than a main meal.
    #[must_use]
    pub const fn is_snack(self) -> bool {
        matches!(self, Self::EveningSnack | Self::Snack)
    }

    /// The carbohydrate floor of this meal, g/kg, on a day whose target is
    /// `day_g_per_kg`: the larger of [`MEAL_CARB_FLOOR_G_PER_KG`] and
    /// [`MEAL_CARB_FLOOR_SHARE`] of the day for a main meal, half of that
    /// for a snack.
    ///
    /// ```
    /// use zerofit_fueling::MealKind;
    /// assert_eq!(MealKind::Dinner.carb_floor_g_per_kg(3.0), 0.5);
    /// assert_eq!(MealKind::Dinner.carb_floor_g_per_kg(9.6), 0.96);
    /// assert_eq!(MealKind::EveningSnack.carb_floor_g_per_kg(9.6), 0.48);
    /// ```
    #[must_use]
    pub fn carb_floor_g_per_kg(self, day_g_per_kg: f64) -> f64 {
        let main = (MEAL_CARB_FLOOR_SHARE * day_g_per_kg).max(MEAL_CARB_FLOOR_G_PER_KG);
        if self.is_snack() { main / 2.0 } else { main }
    }
}

/// Smallest carbohydrate floor of a main meal, g/kg (a snack gets half).
///
/// Judgment call. The consensus gives daily totals and session feeds,
/// not per-meal minimums; but when the session feeds take most of a
/// heavy day's carbohydrate, spreading only the rest leaves dinner at a
/// token amount, which no practitioner would write. 0.5 g/kg is a modest
/// plate of rice or pasta (~35 g for 70 kg).
pub const MEAL_CARB_FLOOR_G_PER_KG: f64 = 0.5;

/// Share of the day's carbohydrate target that a main meal gets at least
/// (a snack half of it): on a 10 g/kg day a dinner has at least 1 g/kg.
/// Judgment call, same reasoning as [`MEAL_CARB_FLOOR_G_PER_KG`]; the
/// flexible pre-ride amount gives way first.
pub const MEAL_CARB_FLOOR_SHARE: f64 = 0.1;

/// Protein per feeding, g/kg: every meal and protein feed gets at least
/// this much. ~0.3 g/kg maximally stimulates muscle protein synthesis in
/// young adults (Moore et al. 2015), and the ISSN position stand advises
/// 0.25–0.4 g/kg per feeding every 3–4 h (Jäger et al. 2017, *J Int Soc
/// Sports Nutr* 14:20).
pub const PROTEIN_PER_FEEDING_G_PER_KG: f64 = 0.3;

/// Protein above which one feeding is too much, g/kg: the top of the
/// ISSN's 0.25–0.4 g/kg per feeding (Jäger et al. 2017). A day needing
/// more gets an extra protein snack in its longest gap.
pub const MAX_PROTEIN_PER_FEEDING_G_PER_KG: f64 = 0.4;

/// A gap between feedings longer than this (minutes, awake and not
/// riding) can take an extra protein snack: the ISSN advises feedings
/// every 3–4 h.
pub const SNACK_GAP_MIN: u32 = 210;

/// A session starting within this many minutes of waking turns breakfast
/// into the pre-ride meal.
pub const EARLY_SESSION_WINDOW_MIN: u32 = 150;

/// A pre-ride meal above this many g/kg is split into a meal and a top-up.
pub const SPLIT_PRE_RIDE_ABOVE_G_PER_KG: f64 = 2.0;

/// Share of a split pre-ride meal eaten as the top-up.
pub const TOP_UP_SHARE: f64 = 0.25;

/// The top-up is eaten this many minutes before the start.
pub const TOP_UP_LEAD_MIN: u32 = 60;

/// Minutes after an evening session ends that the post-ride dinner is
/// served (rounded up to the quarter hour).
pub const POST_RIDE_DINNER_DELAY_MIN: u32 = 30;

/// The evening snack is dropped when it would come this soon after a
/// post-ride dinner.
pub const SNACK_AFTER_DINNER_MIN: u32 = 90;

/// Grams are rounded to multiples of this.
pub const GRAM_STEP: f64 = 5.0;

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

/// Which part of a pre-session meal an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PrePart {
    /// The meal itself (or the only pre-session feed).
    Meal,
    /// The smaller top-up about an hour before the start, when a large
    /// pre-ride target is split.
    TopUp,
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
    /// The pre-session meal or its top-up.
    PreSession {
        /// Index of the session in the day.
        session: usize,
        /// Meal or top-up.
        part: PrePart,
        /// Whether this meal takes breakfast's place (an early session).
        replaces_breakfast: bool,
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

impl EntryKind {
    /// The meal this entry is, if it is a regular meal.
    #[must_use]
    pub const fn meal(&self) -> Option<MealKind> {
        match *self {
            Self::Meal { meal } => Some(meal),
            _ => None,
        }
    }
}

/// One item of the day timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Entry {
    /// Minutes after midnight, a multiple of 15 (may exceed 1440 for
    /// recovery after a late session).
    pub time_min: u32,
    /// What it is.
    pub kind: EntryKind,
    /// Carbohydrate, g, a multiple of 5.
    pub carbs_g: f64,
    /// Protein, g, a multiple of 5.
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
    /// Pre-session meal: lead time (a multiple of 0.5 h) and the
    /// carbohydrate actually planned, which is below 1 g/kg per hour of
    /// lead time only when the day's budget is short.
    pub pre: PreRide,
    /// Whether the pre-session carbohydrate is split into a meal and a
    /// top-up an hour before.
    pub pre_split: bool,
    /// Whether the pre-session meal replaces breakfast.
    pub pre_replaces_breakfast: bool,
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
    /// Carbohydrate in the timeline, g: the target rounded to 5 g, unless
    /// the fixed feeds exceed it (`session_feeds_exceed_target`).
    pub carbs_planned_g: f64,
    /// Protein target, g/kg.
    pub protein_g_per_kg: f64,
    /// Protein target, g (all of it is in the timeline, rounded to 5 g).
    pub protein_g: f64,
    /// Whether the fixed feeds (in-ride, a 1 g/kg pre-ride snack, the
    /// first recovery hour and the meal floors) exceed the daily
    /// carbohydrate target; the session rules then take precedence and
    /// the day is planned above target.
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
/// 1. **Targets**: [`day_load`] → [`daily_carbs_g_per_kg`] and
///    [`daily_protein_g_per_kg`], times body mass. Protein is raised if
///    needed so every feeding gets [`PROTEIN_PER_FEEDING_G_PER_KG`].
/// 2. **Meals around the sessions**:
///    - a session within [`EARLY_SESSION_WINDOW_MIN`] of waking, or a
///      pre-ride meal within an hour of breakfast, turns breakfast into
///      the pre-ride meal;
///    - an evening session (dinner falls between an hour before its
///      pre-ride meal and an hour after it ends) moves dinner to 30 min
///      after it ends, as a post-ride dinner that also
///      carries the first recovery feed; the evening snack goes if it
///      would follow within 90 min;
///    - other meals within an hour of a session feed, or during a
///      session, are dropped: the session feed takes their place.
/// 3. **Carbohydrate, in priority order** (the consensus targets are
///    daily totals and session feeds are timing within them, Burke et
///    al. 2011):
///    1. fixed: in-ride feeds ([`during_ride`]), each kept meal's floor
///       ([`MealKind::carb_floor_g_per_kg`]), a 1 g/kg pre-ride minimum
///       and the first recovery hour when the next session is under
///       [`crate::session::RAPID_RECOVERY_HOURS`] away;
///    2. the pre-ride meal up to its full [`pre_ride`] amount;
///    3. further recovery hours ([`recovery`]), as the budget allows,
///       never past the next session's pre-ride meal;
///    4. the rest spread over the meals (a snack gets half a share).
///
///    If the fixed part alone exceeds the target, the plan keeps it and
///    sets `session_feeds_exceed_target`.
/// 4. **Splitting**: a pre-ride meal above
///    [`SPLIT_PRE_RIDE_ABOVE_G_PER_KG`] becomes a meal (75 %) plus a
///    top-up (25 %) an hour before the start.
/// 5. **Protein**: session feeds (a pre-ride *meal* two or more hours
///    out, or one replacing breakfast, and the first recovery feed) get
///    0.3 g/kg; meals get 0.3 g/kg plus an even share of the rest.
/// 6. **Rounding**: times to the quarter hour (lead times to 0.5 h), grams
///    to 5 g with the largest-remainder method, so the timeline still
///    sums to the target rounded to 5 g.
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
///     next_session_start_min: Some(17 * 60), // a second ride at 17:00
///     schedule: MealSchedule::default(),
/// })?;
/// assert!((plan.carbs_planned_g - plan.carbs_target_g).abs() <= 2.5);
/// assert!(plan.sessions[0].during.carbs_g_per_hour >= 60.0); // a 3 h ride
/// assert!(plan.sessions[0].recovery.hours >= 1); // next ride within 8 h
/// assert!(plan.entries.iter().all(|e| e.time_min % 15 == 0));
/// assert!(plan.entries.windows(2).all(|w| w[0].time_min <= w[1].time_min));
/// # Ok::<(), zerofit_fueling::plan::PlanError>(())
/// ```
pub fn day_plan(input: &DayInput<'_>) -> Result<DayPlan, PlanError> {
    validate(input)?;
    let load = day_load(input.sessions, &input.athlete);
    let carbs_g_per_kg = daily_carbs_g_per_kg(load);
    Ok(build(input, load, carbs_g_per_kg))
}

/// Builds the plan for a given daily carbohydrate target (g/kg). Input is
/// already validated.
pub(crate) fn build(input: &DayInput<'_>, load: f64, carbs_g_per_kg: f64) -> DayPlan {
    let athlete = input.athlete;
    let mass = athlete.body_mass_kg;
    let carbs_target_g = carbs_g_per_kg * mass;

    let mut sessions = draft_sessions(input);
    let meals = kept_meals(input, &sessions);
    let meals = add_protein_snacks(input, &sessions, meals, daily_protein_g_per_kg(load));

    // Carbohydrate by priority (step 3).
    let floors: f64 = meals
        .iter()
        .map(|m| m.kind.carb_floor_g_per_kg(carbs_g_per_kg) * mass)
        .sum();
    let during: f64 = sessions.iter().map(|d| d.plan.during_total_g).sum();
    let pre_min: f64 = sessions.iter().map(|_| mass).sum();
    let first_recovery: f64 = sessions
        .iter()
        .filter(|d| d.plan.recovery.hours > 0)
        .map(|d| d.plan.recovery.carbs_g_per_kg_per_hour * mass)
        .sum();
    let fixed = floors + during + pre_min + first_recovery;
    let session_feeds_exceed_target = fixed > carbs_target_g + 1e-9;
    let mut budget = (carbs_target_g - fixed).max(0.0);

    for d in &mut sessions {
        let wanted = (d.plan.pre.carbs_g_per_kg - 1.0).max(0.0) * mass;
        let extra = wanted.min(budget);
        budget -= extra;
        d.plan.pre.carbs_g_per_kg = 1.0 + extra / mass;
        d.plan.pre_split = d.plan.pre.carbs_g_per_kg > SPLIT_PRE_RIDE_ABOVE_G_PER_KG
            && d.pre_time.saturating_add(TOP_UP_LEAD_MIN)
                < d.session.start_min.saturating_sub(TOP_UP_LEAD_MIN);
    }
    recovery_hours(&mut sessions, mass, &mut budget);

    let base_protein = daily_protein_g_per_kg(load);
    let feedings = f64_from_usize(protein_feedings(&sessions).saturating_add(meals.len()));
    let protein_g_per_kg = base_protein
        .max(PROTEIN_PER_FEEDING_G_PER_KG * feedings)
        .min(MAX_PROTEIN_G_PER_KG)
        .max(base_protein);
    let protein_g = protein_g_per_kg * mass;
    // Every feeding gets the same share: at least the per-feeding floor,
    // unless that would pass the 2.0 g/kg/day cap.
    let per_feeding = protein_g / feedings;
    let mut entries = session_entries(input, &sessions, per_feeding);
    push_meals(
        &mut entries,
        &meals,
        mass,
        carbs_g_per_kg,
        budget,
        per_feeding,
    );

    entries.sort_by_key(|e| e.time_min);
    round_grams(&mut entries, |e| &mut e.carbs_g);
    round_grams(&mut entries, |e| &mut e.protein_g);
    let carbs_planned_g = entries.iter().map(|e| e.carbs_g).sum();
    DayPlan {
        load_kj_per_kg: load,
        band: LoadBand::for_load(load),
        carbs_g_per_kg,
        carbs_target_g,
        carbs_planned_g,
        protein_g_per_kg,
        protein_g,
        session_feeds_exceed_target,
        sessions: sessions.into_iter().map(|d| d.plan).collect(),
        entries,
    }
}

/// Protein target cap, g/kg/day: the top of the consensus range (Thomas,
/// Erdman & Burke 2016; Jäger et al. 2017).
const MAX_PROTEIN_G_PER_KG: f64 = 2.0;

pub(crate) fn validate(input: &DayInput<'_>) -> Result<(), PlanError> {
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

/// A session while the plan is being built.
#[derive(Debug, Clone, Copy)]
struct Draft {
    session: PlannedSession,
    plan: SessionPlan,
    /// Time of the pre-ride meal (the top-up, if split, is at
    /// [`TOP_UP_LEAD_MIN`] before the start).
    pre_time: u32,
    /// Hours of recovery wanted before the budget is applied.
    recovery_wanted: u32,
    /// Start of the next session (today or tomorrow), if any.
    next_start: Option<u32>,
    /// When the post-ride dinner follows this session, its time.
    post_ride_dinner: Option<u32>,
}

/// Pre-ride lead times, in-ride feeds and recovery wanted for each
/// session; carbohydrate amounts are settled later.
fn draft_sessions(input: &DayInput<'_>) -> Vec<Draft> {
    let athlete = input.athlete;
    let wake = input.schedule.wake_min;
    let meal_time = |kind| {
        input
            .schedule
            .meals
            .iter()
            .find(|(_, m)| *m == kind)
            .map(|(t, _)| *t)
    };
    let dinner = meal_time(MealKind::Dinner);
    let breakfast = meal_time(MealKind::Breakfast);
    let mut drafts: Vec<Draft> = Vec::with_capacity(input.sessions.len());
    let mut earliest = wake;
    for (index, s) in input.sessions.iter().enumerate() {
        let available_h = f64::from(s.start_min.saturating_sub(earliest)) / 60.0;
        let mut pre = pre_ride(s, &athlete, available_h);
        // Lead time to the half hour, rounded down so it still fits.
        pre.hours_before = (f64::from(floor_u32(pre.hours_before * 2.0)) / 2.0).max(1.0);
        pre.carbs_g_per_kg = pre.hours_before;
        let lead = round_u32(pre.hours_before * 60.0);
        let pre_time = floor15(s.start_min.saturating_sub(lead))
            .max(ceil15(earliest))
            .min(floor15(s.start_min));
        let replaces_breakfast = index == 0
            && (s.start_min <= wake.saturating_add(EARLY_SESSION_WINDOW_MIN)
                || (pre.hours_before >= 2.0
                    && breakfast.is_some_and(|b| b.abs_diff(pre_time) < MEAL_MERGE_WINDOW_MIN)));
        let during = during_ride(s);
        let during_total_g = during.carbs_g_per_hour * f64::from(s.duration_min) / 60.0;
        let next_start = input
            .sessions
            .get(index.saturating_add(1))
            .map(|n| n.start_min)
            .or(input.next_session_start_min);
        let hours_until_next = next_start.map(|n| f64::from(n.saturating_sub(s.end_min())) / 60.0);
        let rec = recovery(s, &athlete, hours_until_next);
        // Evening: dinner falls inside the session's footprint, from an
        // hour before its pre-ride meal to an hour after it ends.
        let is_evening = dinner.is_some_and(|d| {
            d >= pre_time.saturating_sub(MEAL_MERGE_WINDOW_MIN)
                && d < s.end_min().saturating_add(MEAL_MERGE_WINDOW_MIN)
        });
        drafts.push(Draft {
            session: *s,
            plan: SessionPlan {
                start_min: s.start_min,
                duration_min: s.duration_min,
                load_kj_per_kg: session_load(s, &athlete),
                pre,
                pre_split: false,
                pre_replaces_breakfast: replaces_breakfast,
                during,
                during_total_g,
                recovery: rec,
            },
            pre_time,
            recovery_wanted: rec.hours,
            next_start,
            post_ride_dinner: is_evening
                .then(|| ceil15(s.end_min().saturating_add(POST_RIDE_DINNER_DELAY_MIN))),
        });
        earliest = s.end_min();
    }
    drafts
}

/// Recovery hours (step 3.3): the first is fixed, further ones as the
/// budget allows and before the next session's pre-ride meal.
fn recovery_hours(sessions: &mut [Draft], mass: f64, budget: &mut f64) {
    let pre_times: Vec<u32> = sessions.iter().map(|d| d.pre_time).collect();
    for (index, d) in sessions.iter_mut().enumerate() {
        let per_hour_g = d.plan.recovery.carbs_g_per_kg_per_hour * mass;
        if d.recovery_wanted == 0 || per_hour_g <= 0.0 {
            continue;
        }
        let next_pre = pre_times
            .get(index.saturating_add(1))
            .copied()
            .or_else(|| d.next_start.map(|n| n.saturating_sub(60)));
        let first = recovery_start(d);
        let fit = (1..d.recovery_wanted)
            .take_while(|h| {
                let t = first.saturating_add(h.saturating_mul(60));
                next_pre.is_none_or(|p| t < p)
            })
            .count();
        let affordable = floor_u32(*budget / per_hour_g);
        let extra = affordable.min(u32::try_from(fit).unwrap_or(u32::MAX));
        *budget -= per_hour_g * f64::from(extra);
        d.plan.recovery.hours = extra.saturating_add(1);
    }
}

/// When recovery starts: the post-ride dinner, or the session end rounded
/// up to the quarter hour.
fn recovery_start(d: &Draft) -> u32 {
    d.post_ride_dinner
        .unwrap_or_else(|| ceil15(d.session.end_min()))
}

/// A regular meal that stays in the plan.
#[derive(Debug, Clone, Copy)]
struct KeptMeal {
    time_min: u32,
    kind: MealKind,
}

/// Regular meals not displaced by a session, plus post-ride dinners
/// (step 2).
fn kept_meals(input: &DayInput<'_>, sessions: &[Draft]) -> Vec<KeptMeal> {
    let post_dinner = sessions
        .iter()
        .filter_map(|d| d.post_ride_dinner)
        .next_back();
    let feed_times: Vec<u32> = sessions
        .iter()
        .flat_map(|d| {
            let rec = (d.post_ride_dinner.is_none()).then(|| ceil15(d.session.end_min()));
            [Some(d.pre_time), rec]
        })
        .flatten()
        .collect();
    let mut meals: Vec<KeptMeal> = input
        .schedule
        .meals
        .iter()
        .filter(|&&(t, kind)| {
            if kind == MealKind::Breakfast && sessions.iter().any(|d| d.plan.pre_replaces_breakfast)
            {
                return false;
            }
            if kind == MealKind::Dinner && post_dinner.is_some() {
                return false;
            }
            if kind == MealKind::EveningSnack
                && post_dinner.is_some_and(|p| t < p.saturating_add(SNACK_AFTER_DINNER_MIN))
            {
                return false;
            }
            let near_feed = feed_times
                .iter()
                .any(|f| t.abs_diff(*f) < MEAL_MERGE_WINDOW_MIN);
            let in_session = sessions
                .iter()
                .any(|d| t >= d.session.start_min.saturating_sub(30) && t < d.session.end_min());
            !near_feed && !in_session
        })
        .map(|&(t, kind)| KeptMeal {
            time_min: ceil15(t),
            kind,
        })
        .collect();
    if let Some(time_min) = post_dinner {
        meals.push(KeptMeal {
            time_min,
            kind: MealKind::PostRideDinner,
        });
    }
    if meals.is_empty() {
        // Every meal was displaced: one evening meal after the last feed.
        let last = sessions
            .iter()
            .map(|d| ceil15(d.session.end_min()))
            .max()
            .unwrap_or(0);
        meals.push(KeptMeal {
            time_min: ceil15(last.saturating_add(MEAL_MERGE_WINDOW_MIN).max(1110)),
            kind: MealKind::Dinner,
        });
    }
    meals
}

/// Pre-ride, in-ride and recovery entries with their final amounts.
/// Recovery hour 0 of a session followed by a post-ride dinner is
/// carried by that dinner (see [`push_meals`]) and not listed here.
fn session_entries(input: &DayInput<'_>, sessions: &[Draft], protein_feed: f64) -> Vec<Entry> {
    let mass = input.athlete.body_mass_kg;
    let mut entries = Vec::new();
    for (index, d) in sessions.iter().enumerate() {
        let s = &d.session;
        let pre_g = d.plan.pre.carbs_g_per_kg * mass;
        let is_meal = pre_is_meal(d);
        let split = d.plan.pre_split;
        let meal_g = if split {
            pre_g * (1.0 - TOP_UP_SHARE)
        } else {
            pre_g
        };
        entries.push(Entry {
            time_min: d.pre_time,
            kind: EntryKind::PreSession {
                session: index,
                part: PrePart::Meal,
                replaces_breakfast: d.plan.pre_replaces_breakfast,
            },
            carbs_g: meal_g,
            // A meal two or more hours out is a real meal with protein; a
            // snack closer to the start stays carbohydrate-only.
            protein_g: if is_meal { protein_feed } else { 0.0 },
        });
        if split {
            entries.push(Entry {
                time_min: floor15(s.start_min.saturating_sub(TOP_UP_LEAD_MIN)),
                kind: EntryKind::PreSession {
                    session: index,
                    part: PrePart::TopUp,
                    replaces_breakfast: false,
                },
                carbs_g: pre_g - meal_g,
                protein_g: 0.0,
            });
        }
        push_during_feeds(
            &mut entries,
            index,
            s,
            &d.plan.during,
            d.plan.during_total_g,
        );
        push_recovery_feeds(&mut entries, index, d, mass, protein_feed);
    }
    entries
}

/// Whether a session's pre-ride meal is a real meal with protein: two or
/// more hours out, or replacing breakfast. A snack closer to the start
/// stays carbohydrate-only.
fn pre_is_meal(d: &Draft) -> bool {
    d.plan.pre.hours_before >= 2.0 || d.plan.pre_replaces_breakfast
}

/// Protein feedings from the sessions: pre-ride meals, and the first
/// recovery feed unless a post-ride dinner carries it.
fn protein_feedings(sessions: &[Draft]) -> usize {
    sessions
        .iter()
        .map(|d| {
            usize::from(pre_is_meal(d)).saturating_add(usize::from(d.post_ride_dinner.is_none()))
        })
        .fold(0, usize::saturating_add)
}

/// Adds a protein [`MealKind::Snack`] in the longest gap between
/// feedings while one feeding would need more than
/// [`MAX_PROTEIN_PER_FEEDING_G_PER_KG`] (at most three snacks). Gaps
/// count only time awake and off the bike; the snack goes in the middle,
/// on a quarter hour.
fn add_protein_snacks(
    input: &DayInput<'_>,
    sessions: &[Draft],
    mut meals: Vec<KeptMeal>,
    protein_g_per_kg: f64,
) -> Vec<KeptMeal> {
    let session_feedings = protein_feedings(sessions);
    for _ in 0..3 {
        let feedings = f64_from_usize(session_feedings.saturating_add(meals.len()));
        if protein_g_per_kg / feedings <= MAX_PROTEIN_PER_FEEDING_G_PER_KG + 1e-9 {
            break;
        }
        let mut times: Vec<u32> = meals.iter().map(|m| m.time_min).collect();
        for d in sessions {
            if pre_is_meal(d) {
                times.push(d.pre_time);
            }
            times.push(recovery_start(d));
        }
        times.push(ceil15(input.schedule.wake_min));
        times.sort_unstable();
        let riding = |a: u32, b: u32| {
            sessions
                .iter()
                .any(|d| d.session.start_min < b && d.session.end_min() > a)
        };
        let gap = times
            .windows(2)
            .filter_map(|w| match *w {
                [a, b] if b.saturating_sub(a) > SNACK_GAP_MIN && !riding(a, b) => Some((a, b)),
                _ => None,
            })
            .max_by_key(|(a, b)| b.saturating_sub(*a));
        let Some((a, b)) = gap else { break };
        meals.push(KeptMeal {
            time_min: floor15(a.saturating_add(b.saturating_sub(a) / 2)),
            kind: MealKind::Snack,
        });
    }
    meals
}

/// Spreads the remaining carbohydrate (after floors, weighted) over the
/// kept meals, each with `protein` g. A post-ride dinner also carries its
/// session's first recovery feed. `day_g_per_kg` is the day's
/// carbohydrate target (for the floors).
fn push_meals(
    entries: &mut Vec<Entry>,
    meals: &[KeptMeal],
    mass: f64,
    day_g_per_kg: f64,
    carbs_left: f64,
    protein: f64,
) {
    let weights: f64 = meals.iter().map(|m| m.kind.carb_weight()).sum();
    for m in meals {
        entries.push(Entry {
            time_min: m.time_min,
            kind: EntryKind::Meal { meal: m.kind },
            carbs_g: m.kind.carb_floor_g_per_kg(day_g_per_kg) * mass
                + if weights > 0.0 {
                    carbs_left * m.kind.carb_weight() / weights
                } else {
                    0.0
                },
            protein_g: protein,
        });
    }
}

/// Feeds every `feed_interval_min` from the first interval until before
/// the end (times rounded down to the quarter hour), sharing `total_g`
/// equally.
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
        let exact = s
            .start_min
            .saturating_add(k.saturating_mul(during.feed_interval_min));
        entries.push(Entry {
            time_min: floor15(exact).max(ceil15(s.start_min.saturating_add(1))),
            kind: EntryKind::DuringSession { session: index },
            carbs_g: per_feed,
            protein_g: 0.0,
        });
    }
}

/// Hourly recovery feeds from the session end; or, without rapid
/// recovery, one protein feed at the end. With a post-ride dinner, the
/// dinner is hour 0 and only the later hours are listed.
fn push_recovery_feeds(entries: &mut Vec<Entry>, index: usize, d: &Draft, mass: f64, protein: f64) {
    let rec = &d.plan.recovery;
    let start = recovery_start(d);
    let first_hour = u32::from(d.post_ride_dinner.is_some());
    if rec.hours == 0 {
        if d.post_ride_dinner.is_none() {
            entries.push(Entry {
                time_min: start,
                kind: EntryKind::Recovery {
                    session: index,
                    hour: 0,
                },
                carbs_g: 0.0,
                protein_g: protein,
            });
        }
        return;
    }
    for hour in 0..rec.hours {
        let carbs = rec.carbs_g_per_kg_per_hour * mass;
        if hour < first_hour {
            // Carried by the post-ride dinner: add it to that entry later.
            entries.push(Entry {
                time_min: start,
                kind: EntryKind::Meal {
                    meal: MealKind::PostRideDinner,
                },
                carbs_g: carbs,
                protein_g: 0.0,
            });
            continue;
        }
        entries.push(Entry {
            time_min: start.saturating_add(hour.saturating_mul(60)),
            kind: EntryKind::Recovery {
                session: index,
                hour,
            },
            carbs_g: carbs,
            protein_g: if hour == 0 { protein } else { 0.0 },
        });
    }
}

/// Rounds one quantity of every entry to [`GRAM_STEP`] with the
/// largest-remainder method: the total becomes the exact total rounded to
/// the step, and no entry moves by a full step. Entries for the same meal
/// (a post-ride dinner's recovery share) are merged first.
fn round_grams(entries: &mut Vec<Entry>, field: fn(&mut Entry) -> &mut f64) {
    merge_duplicates(entries);
    let mut total_exact = 0.0;
    let mut remainders: Vec<(usize, f64)> = Vec::with_capacity(entries.len());
    let mut total_floored = 0.0;
    for (i, e) in entries.iter_mut().enumerate() {
        let v = *field(e);
        total_exact += v;
        let units = v / GRAM_STEP;
        let floored = f64::from(floor_u32(units));
        total_floored += floored;
        remainders.push((i, units - floored));
        *field(e) = floored * GRAM_STEP;
    }
    let target_units = f64::from(round_u32(total_exact / GRAM_STEP));
    let mut missing = floor_u32(target_units - total_floored);
    remainders.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    for (i, _) in remainders {
        if missing == 0 {
            break;
        }
        if let Some(e) = entries.get_mut(i) {
            *field(e) += GRAM_STEP;
            missing = missing.saturating_sub(1);
        }
    }
}

/// Merges entries with the same kind and time (a post-ride dinner and the
/// recovery carbohydrate it carries).
fn merge_duplicates(entries: &mut Vec<Entry>) {
    let mut merged: Vec<Entry> = Vec::with_capacity(entries.len());
    for e in entries.drain(..) {
        if let Some(m) = merged
            .iter_mut()
            .find(|m| m.kind == e.kind && m.time_min == e.time_min)
        {
            m.carbs_g += e.carbs_g;
            m.protein_g += e.protein_g;
        } else {
            merged.push(e);
        }
    }
    merged.sort_by_key(|e| e.time_min);
    *entries = merged;
}

/// `t` rounded down to the quarter hour.
const fn floor15(t: u32) -> u32 {
    match t.checked_rem(15) {
        Some(r) => t.saturating_sub(r),
        None => t,
    }
}

/// `t` rounded up to the quarter hour.
const fn ceil15(t: u32) -> u32 {
    floor15(t.saturating_add(14))
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

    fn meal(p: &DayPlan, kind: MealKind) -> Option<Entry> {
        p.entries
            .iter()
            .copied()
            .find(|e| e.kind.meal() == Some(kind))
    }

    fn sums_to_target(p: &DayPlan) {
        assert!(
            (p.carbs_planned_g - p.carbs_target_g).abs() <= GRAM_STEP / 2.0 + 1e-9,
            "{} vs {}",
            p.carbs_planned_g,
            p.carbs_target_g
        );
        let protein: f64 = p.entries.iter().map(|e| e.protein_g).sum();
        assert!((protein - p.protein_g).abs() <= GRAM_STEP / 2.0 + 1e-9);
    }

    fn well_formed(p: &DayPlan) {
        for e in &p.entries {
            assert_eq!(e.time_min % 15, 0, "{e:?}");
            assert_eq!(e.carbs_g % GRAM_STEP, 0.0, "{e:?}");
            assert_eq!(e.protein_g % GRAM_STEP, 0.0, "{e:?}");
        }
        assert!(p.entries.windows(2).all(|w| w[0].time_min <= w[1].time_min));
    }

    #[test]
    fn rest_day_is_meals_only() {
        let p = plan(&[], None);
        well_formed(&p);
        sums_to_target(&p);
        assert_eq!(p.band, LoadBand::Light);
        assert!((p.carbs_target_g - 210.0).abs() < 1e-9); // 3 g/kg
        assert!((p.protein_g - 84.0).abs() < 1e-9); // 1.2 g/kg
        assert_eq!(p.entries.len(), 4);
        // Floors (35/17.5 g) plus an even share: 60, 60, 60, 30 g.
        assert_eq!(p.entries[0].carbs_g, 60.0);
        assert_eq!(p.entries[3].carbs_g, 30.0);
        // 21 g protein each, rounded to 5 g and summing to 85 g.
        assert_eq!(p.entries.iter().map(|e| e.protein_g).sum::<f64>(), 85.0);
    }

    #[test]
    fn evening_session_gets_a_post_ride_dinner() {
        // The demo's Thursday: 18:00, 65 min at IF 0.8.
        let p = plan(&[PlannedSession::new(18 * 60, 65, 0.8)], None);
        well_formed(&p);
        sums_to_target(&p);
        assert!(meal(&p, MealKind::Dinner).is_none());
        let dinner = meal(&p, MealKind::PostRideDinner).unwrap();
        assert_eq!(dinner.time_min, 19 * 60 + 45); // 19:05 + 30 min, rounded up
        assert!(
            dinner.carbs_g
                >= MealKind::Dinner.carb_floor_g_per_kg(p.carbs_g_per_kg) * 70.0 - GRAM_STEP
        );
        assert!(dinner.protein_g >= 20.0);
        // The snack at 21:30 is 105 min later: kept.
        assert!(meal(&p, MealKind::EveningSnack).is_some());
        // No separate protein-only recovery feed: dinner is the recovery.
        assert!(
            !p.entries
                .iter()
                .any(|e| matches!(e.kind, EntryKind::Recovery { .. }))
        );
    }

    #[test]
    fn late_evening_session_drops_the_snack() {
        let p = plan(&[PlannedSession::new(19 * 60 + 30, 90, 0.7)], None);
        let dinner = meal(&p, MealKind::PostRideDinner).unwrap();
        assert_eq!(dinner.time_min, 21 * 60 + 30);
        assert!(meal(&p, MealKind::EveningSnack).is_none());
        sums_to_target(&p);
    }

    #[test]
    fn early_session_turns_breakfast_into_the_pre_ride_meal() {
        let p = plan(&[PlannedSession::new(7 * 60 + 30, 120, 0.7)], None);
        well_formed(&p);
        assert!(meal(&p, MealKind::Breakfast).is_none());
        let pre = p
            .entries
            .iter()
            .find(|e| matches!(e.kind, EntryKind::PreSession { .. }))
            .unwrap();
        assert_eq!(
            pre.kind,
            EntryKind::PreSession {
                session: 0,
                part: PrePart::Meal,
                replaces_breakfast: true
            }
        );
        assert_eq!(pre.time_min, 6 * 60 + 30); // at waking: 1 h lead
        assert!(pre.protein_g > 0.0); // a breakfast has protein
        assert!(p.sessions[0].pre_replaces_breakfast);
    }

    #[test]
    fn large_pre_ride_meal_is_split() {
        // 11:00, 4 h: a 3-4 g/kg pre-ride target.
        let p = plan(&[PlannedSession::new(11 * 60, 240, 0.75)], None);
        well_formed(&p);
        assert!(p.sessions[0].pre_split);
        let parts: Vec<_> = p
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::PreSession { .. }))
            .collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1].time_min, 10 * 60);
        assert!(parts[1].carbs_g < parts[0].carbs_g);
        assert!(matches!(
            parts[1].kind,
            EntryKind::PreSession {
                part: PrePart::TopUp,
                ..
            }
        ));
        // Lead times are whole half hours.
        assert_eq!(p.sessions[0].pre.hours_before % 0.5, 0.0);
    }

    #[test]
    fn heavy_day_meals_keep_their_floors() {
        // The demo's Saturday: 09:00, 4 h 45 min, another ride tomorrow.
        let p = plan(&[PlannedSession::new(9 * 60, 285, 0.7)], Some(1440 + 570));
        well_formed(&p);
        for e in &p.entries {
            if let Some(m) = e.kind.meal() {
                assert!(
                    e.carbs_g >= m.carb_floor_g_per_kg(p.carbs_g_per_kg) * 70.0 - GRAM_STEP,
                    "{e:?}"
                );
                assert!(e.protein_g >= 20.0, "{e:?}");
            }
        }
        // Protein evenly spread: no meal over 0.45 g/kg.
        assert!(
            p.entries
                .iter()
                .all(|e| e.protein_g <= 0.45 * 70.0 + GRAM_STEP)
        );
    }

    #[test]
    fn long_ride_timeline() {
        // 09:00, 3 h at IF 0.75; a second ride at 17:00 (5 h later).
        let p = plan(&[PlannedSession::new(540, 180, 0.75)], Some(17 * 60));
        well_formed(&p);
        sums_to_target(&p);
        let s = p.sessions[0];
        assert!(s.during.carbs_g_per_hour >= 60.0);
        let during: Vec<_> = p
            .entries
            .iter()
            .filter(|e| matches!(e.kind, EntryKind::DuringSession { .. }))
            .collect();
        assert_eq!(during.len(), 5); // 30, 60, ..., 150 min
        assert!(s.recovery.hours >= 1);
        assert!(!p.session_feeds_exceed_target);
        assert!(meal(&p, MealKind::Lunch).is_none());
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
        assert_eq!(rec[0].protein_g, 20.0);
    }

    #[test]
    fn double_day_recovery_stops_at_next_pre_meal() {
        // 07:00 2 h, then 15:00 1 h.
        let sessions = [
            PlannedSession::new(420, 120, 0.8),
            PlannedSession::new(900, 60, 0.9),
        ];
        let p = plan(&sessions, None);
        well_formed(&p);
        let pre2 = p
            .entries
            .iter()
            .find(|e| matches!(e.kind, EntryKind::PreSession { session: 1, .. }))
            .unwrap()
            .time_min;
        assert!(p.entries.iter().all(|e| match e.kind {
            EntryKind::Recovery { session: 0, .. } => e.time_min < pre2,
            _ => true,
        }));
        // First session's pre-meal can't be before waking: 30 min lead
        // available, so 1 h / 1 g/kg at waking.
        assert_eq!(p.sessions[0].pre.hours_before, 1.0);
    }

    #[test]
    fn feeds_exceeding_target_are_flagged() {
        // 7 h hard ride and another long ride tomorrow morning.
        let p = plan(&[PlannedSession::new(480, 420, 0.95)], Some(1440 + 420));
        assert_eq!(p.band, LoadBand::VeryHigh);
        if p.session_feeds_exceed_target {
            assert!(p.carbs_planned_g >= p.carbs_target_g - GRAM_STEP);
        } else {
            sums_to_target(&p);
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
        assert!(meal(&p, MealKind::Dinner).is_some());
        sums_to_target(&p);
    }

    #[test]
    fn quarter_hours() {
        assert_eq!(floor15(943), 930);
        assert_eq!(ceil15(943), 945);
        assert_eq!(ceil15(945), 945);
        assert_eq!(floor15(u32::MAX - 1), u32::MAX - 15);
        assert_eq!(ceil15(u32::MAX - 1), u32::MAX);
    }
}
