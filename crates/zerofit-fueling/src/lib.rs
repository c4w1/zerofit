//! Rules-based carbohydrate and protein periodization for endurance
//! training.
//!
//! Given a day's planned (or completed) sessions, `zerofit-fueling`
//! produces a fueling plan: daily carbohydrate and protein targets scaled
//! by the training load, a pre-session meal, in-session carbohydrate per
//! hour, recovery feeds when the next session is close, and per-meal
//! targets for the rest of the day.
//!
//! Every rule comes from published sports-nutrition consensus and is
//! cited where it is implemented:
//!
//! - Thomas DT, Erdman KA, Burke LM. *American College of Sports Medicine
//!   Joint Position Statement: Nutrition and Athletic Performance.* Med
//!   Sci Sports Exerc 2016;48(3):543–568 (also J Acad Nutr Diet, Can J
//!   Diet Pract Res).
//! - Burke LM, Hawley JA, Wong SHS, Jeukendrup AE. *Carbohydrates for
//!   training and competition.* J Sports Sci 2011;29(S1):S17–S27 (IOC
//!   consensus on sports nutrition 2010/11).
//! - Jeukendrup A. *A step towards personalized sports nutrition:
//!   carbohydrate intake during exercise.* Sports Med 2014;44(S1):S25–S33.
//! - Moore DR et al. *Protein ingestion to stimulate myofibrillar protein
//!   synthesis requires greater relative protein intakes in healthy older
//!   versus younger men.* J Gerontol A 2015;70(1):57–62 (~0.3 g/kg per
//!   meal); Areta JL et al., J Physiol 2013 (protein distribution).
//!
//! **This is general guidance for healthy athletes, not medical or
//! dietary advice.** The rules are deterministic and transparent by
//! design, so every number in a plan can be traced to a formula and a
//! source; there is no learned model.
//!
//! # Example
//!
//! ```
//! use zerofit_fueling::{Athlete, PlannedSession, day_plan, DayInput, MealSchedule};
//!
//! let athlete = Athlete { body_mass_kg: 70.0, ftp_w: Some(260.0) };
//! // A 2.5 h endurance ride at 08:30 with 3 × 10 min at threshold.
//! let sessions = [PlannedSession { work_kj: Some(1900.0), ..PlannedSession::new(510, 150, 0.78) }];
//! let plan = day_plan(&DayInput {
//!     athlete,
//!     sessions: &sessions,
//!     next_session_start_min: Some(24 * 60 + 18 * 60), // tomorrow 18:00
//!     schedule: MealSchedule::default(),
//! })?;
//! println!("{:.1} g/kg carbohydrate ({:.0} g), {:.0} g protein",
//!          plan.carbs_g_per_kg, plan.carbs_target_g, plan.protein_g);
//! for e in &plan.entries {
//!     println!("{:02}:{:02} {:?}: {:.0} g carbs, {:.0} g protein",
//!              e.time_min / 60 % 24, e.time_min % 60, e.kind, e.carbs_g, e.protein_g);
//! }
//! # Ok::<(), zerofit_fueling::PlanError>(())
//! ```
//!
//! # `no_std`
//!
//! `#![no_std]` with `alloc` (a plan's timeline has a variable number of
//! entries), no dependencies, panic-free under the workspace lints.
#![no_std]
// Exact float comparisons are intended in tests of hand-computed values.
#![cfg_attr(test, allow(clippy::float_cmp))]

extern crate alloc;

pub mod daily;
mod num;
pub mod plan;
pub mod session;

pub use daily::{LoadBand, daily_carbs_g_per_kg, daily_protein_g_per_kg, day_load};
pub use plan::{
    DayInput, DayPlan, Entry, EntryKind, MealKind, MealSchedule, PlanError, PrePart, day_plan,
};

/// The athlete, as far as fueling cares.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Athlete {
    /// Body mass, kg. Every target is per kg.
    pub body_mass_kg: f64,
    /// FTP, W; used to estimate work for sessions without planned kJ.
    pub ftp_w: Option<f64>,
}

/// A planned (or completed) training session.
///
/// ```
/// use zerofit_fueling::PlannedSession;
/// let s = PlannedSession::new(7 * 60, 90, 0.7);
/// assert_eq!(s.end_min(), 8 * 60 + 30);
/// assert_eq!(s.work_kj, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PlannedSession {
    /// Start, minutes after midnight.
    pub start_min: u32,
    /// Duration, minutes.
    pub duration_min: u32,
    /// Intensity factor (NP / FTP) of the session.
    pub intensity_factor: f64,
    /// Planned mechanical work, kJ, if known (from a workout or a
    /// completed ride). Otherwise it is estimated from duration, IF and
    /// FTP.
    pub work_kj: Option<f64>,
}

impl PlannedSession {
    /// A session without planned work.
    #[must_use]
    pub const fn new(start_min: u32, duration_min: u32, intensity_factor: f64) -> Self {
        Self {
            start_min,
            duration_min,
            intensity_factor,
            work_kj: None,
        }
    }

    /// End, minutes after midnight.
    #[must_use]
    pub const fn end_min(&self) -> u32 {
        self.start_min.saturating_add(self.duration_min)
    }
}
