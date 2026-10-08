//! Structured workouts: steps at a percentage of FTP, their planned load,
//! and export to Zwift (`.zwo`) and FIT workout files.
//!
//! A [`Workout`] is a flat list of [`WorkoutStep`]s, each a duration and a
//! target that is either steady or a linear ramp, as a percentage of FTP.
//! Repeats are expanded by the caller (a builder UI repeats the steps).
//!
//! The planned metrics expand the workout to the 1 Hz power a rider
//! hitting every target exactly would produce, then run the same
//! [`normalized_power`] and [`tss`] code as a recorded ride, so planned
//! and completed load are directly comparable.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use crate::num::{f64_from_usize, round_u16};
use crate::power::{average_power, normalized_power, tss, work_kj};

/// What a step is for. Maps to the FIT `intensity` enum and to Zwift's
/// warm-up / cool-down blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum StepKind {
    /// Warm-up.
    Warmup,
    /// Work interval or steady riding (default).
    #[default]
    Active,
    /// Recovery between intervals.
    Recovery,
    /// Cool-down.
    Cooldown,
}

/// A step's power target, in percent of FTP.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum StepTarget {
    /// Hold one power.
    Steady {
        /// Target, % FTP.
        pct_ftp: f64,
    },
    /// Change power linearly from `from_pct` to `to_pct`.
    Ramp {
        /// Power at the start, % FTP.
        from_pct: f64,
        /// Power at the end, % FTP.
        to_pct: f64,
    },
}

/// One step of a workout.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct WorkoutStep {
    /// Duration, s.
    pub duration_s: u32,
    /// Power target.
    pub target: StepTarget,
    /// What the step is for.
    pub kind: StepKind,
}

impl WorkoutStep {
    /// A steady step.
    ///
    /// ```
    /// use zerofit_analytics::workout::{StepKind, WorkoutStep};
    /// let s = WorkoutStep::steady(300, 105.0, StepKind::Active);
    /// assert_eq!(s.watts_at(0, 200.0), 210.0);
    /// ```
    #[must_use]
    pub const fn steady(duration_s: u32, pct_ftp: f64, kind: StepKind) -> Self {
        Self {
            duration_s,
            target: StepTarget::Steady { pct_ftp },
            kind,
        }
    }

    /// A ramp.
    ///
    /// ```
    /// use zerofit_analytics::workout::{StepKind, WorkoutStep};
    /// let s = WorkoutStep::ramp(600, 50.0, 75.0, StepKind::Warmup);
    /// assert!(s.watts_at(0, 200.0) > 100.0 && s.watts_at(599, 200.0) < 150.0);
    /// ```
    #[must_use]
    pub const fn ramp(duration_s: u32, from_pct: f64, to_pct: f64, kind: StepKind) -> Self {
        Self {
            duration_s,
            target: StepTarget::Ramp { from_pct, to_pct },
            kind,
        }
    }

    /// Target power, W, in second `t` of the step (0-based), for `ftp`.
    /// A ramp is sampled at the middle of each second, so its average is
    /// exactly the mean of its end points.
    #[must_use]
    pub fn watts_at(&self, t: u32, ftp: f64) -> f64 {
        let pct = match self.target {
            StepTarget::Steady { pct_ftp } => pct_ftp,
            StepTarget::Ramp { from_pct, to_pct } => {
                let n = f64::from(self.duration_s.max(1));
                from_pct + (to_pct - from_pct) * (f64::from(t) + 0.5) / n
            }
        };
        pct.max(0.0) / 100.0 * ftp
    }
}

/// A structured workout.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Workout {
    /// Name, shown on the device.
    pub name: String,
    /// Steps, in order.
    pub steps: Vec<WorkoutStep>,
}

/// The planned load of a workout for a given FTP.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct PlannedLoad {
    /// Total duration, s.
    pub duration_s: u32,
    /// Average power, W.
    pub average_power: f64,
    /// Normalized power, W (`None` under 30 s).
    pub normalized_power: Option<f64>,
    /// Intensity factor.
    pub intensity_factor: Option<f64>,
    /// Training stress score over the whole duration.
    pub tss: Option<f64>,
    /// Work, kJ.
    pub work_kj: f64,
}

impl Workout {
    /// Total duration, s.
    #[must_use]
    pub fn duration_s(&self) -> u32 {
        self.steps
            .iter()
            .fold(0u32, |acc, s| acc.saturating_add(s.duration_s))
    }

    /// The 1 Hz power profile of riding every target exactly, W.
    ///
    /// ```
    /// use zerofit_analytics::workout::{StepKind, Workout, WorkoutStep};
    /// let w = Workout {
    ///     name: "test".into(),
    ///     steps: vec![WorkoutStep::steady(2, 50.0, StepKind::Active), WorkoutStep::steady(1, 100.0, StepKind::Active)],
    /// };
    /// assert_eq!(w.power_profile(300.0), vec![150, 150, 300]);
    /// ```
    #[must_use]
    pub fn power_profile(&self, ftp: f64) -> Vec<u16> {
        let total = usize::try_from(self.duration_s()).unwrap_or(0);
        let mut out = Vec::with_capacity(total);
        for step in &self.steps {
            out.extend((0..step.duration_s).map(|t| round_u16(step.watts_at(t, ftp))));
        }
        out
    }

    /// Planned average power, NP, IF, TSS and work for `ftp` (W).
    ///
    /// TSS uses the full duration: a workout has no stops, so moving and
    /// recording time are the same. `None` metrics for an FTP ≤ 0.
    ///
    /// ```
    /// use zerofit_analytics::workout::{StepKind, Workout, WorkoutStep};
    /// // One hour at 100 % FTP is 100 TSS by definition.
    /// let w = Workout { name: "FTP hour".into(), steps: vec![WorkoutStep::steady(3600, 100.0, StepKind::Active)] };
    /// let load = w.planned_load(250.0);
    /// assert!((load.tss.unwrap() - 100.0).abs() < 1e-9);
    /// assert_eq!(load.work_kj, 900.0);
    /// ```
    #[must_use]
    pub fn planned_load(&self, ftp: f64) -> PlannedLoad {
        let profile = self.power_profile(ftp.max(0.0));
        let np = normalized_power(&profile);
        let ftp = Some(ftp).filter(|f| *f > 0.0);
        PlannedLoad {
            duration_s: self.duration_s(),
            average_power: average_power(&profile).unwrap_or(0.0),
            normalized_power: np,
            intensity_factor: np.zip(ftp).map(|(np, ftp)| np / ftp),
            tss: np
                .zip(ftp)
                .and_then(|(np, ftp)| tss(f64_from_usize(profile.len()), np, ftp)),
            work_kj: work_kj(&profile),
        }
    }

    /// The workout as a Zwift `.zwo` file.
    ///
    /// Steady steps become `SteadyState`; ramps become `Warmup` or
    /// `Cooldown` when the step kind says so, else `Ramp`. Zwift stores
    /// power as a fraction of FTP and, for ramps, reads `PowerLow` as the
    /// start and `PowerHigh` as the end whichever is larger. The name is
    /// XML-escaped.
    ///
    /// ```
    /// use zerofit_analytics::workout::{StepKind, Workout, WorkoutStep};
    /// let w = Workout {
    ///     name: "3x5 <VO2>".into(),
    ///     steps: vec![
    ///         WorkoutStep::ramp(600, 50.0, 75.0, StepKind::Warmup),
    ///         WorkoutStep::steady(300, 115.0, StepKind::Active),
    ///     ],
    /// };
    /// let xml = w.to_zwo();
    /// assert!(xml.contains("<name>3x5 &lt;VO2&gt;</name>"));
    /// assert!(xml.contains(r#"<Warmup Duration="600" PowerLow="0.500" PowerHigh="0.750"/>"#));
    /// assert!(xml.contains(r#"<SteadyState Duration="300" Power="1.150"/>"#));
    /// ```
    #[must_use]
    pub fn to_zwo(&self) -> String {
        let mut x = String::new();
        // Writing to a String cannot fail.
        let _ = write!(
            x,
            "<workout_file>\n  <author>zerofit</author>\n  <name>{}</name>\n  \
             <description>Exported from zerofit</description>\n  \
             <sportType>bike</sportType>\n  <tags/>\n  <workout>\n",
            escape_xml(&self.name)
        );
        for step in &self.steps {
            let _ = match step.target {
                StepTarget::Steady { pct_ftp } => writeln!(
                    x,
                    "    <SteadyState Duration=\"{}\" Power=\"{:.3}\"/>",
                    step.duration_s,
                    pct_ftp / 100.0
                ),
                StepTarget::Ramp { from_pct, to_pct } => {
                    let tag = match step.kind {
                        StepKind::Warmup => "Warmup",
                        StepKind::Cooldown => "Cooldown",
                        StepKind::Active | StepKind::Recovery => "Ramp",
                    };
                    writeln!(
                        x,
                        "    <{tag} Duration=\"{}\" PowerLow=\"{:.3}\" PowerHigh=\"{:.3}\"/>",
                        step.duration_s,
                        from_pct / 100.0,
                        to_pct / 100.0
                    )
                }
            };
        }
        x.push_str("  </workout>\n</workout_file>\n");
        x
    }

    /// The workout as a FIT workout file (feature `fit`).
    ///
    /// Writes `file_id` (type workout), one `workout` message and one
    /// `workout_step` per step: a time duration and a custom power target
    /// in % FTP. `time_created` is FIT time (s since 1989-12-31). See
    /// [`fit_workout`] for the encoding.
    ///
    /// # Errors
    ///
    /// An encoder error, which only a name longer than 254 bytes can cause
    /// (it is truncated to 31 bytes first, so in practice none).
    #[cfg(feature = "fit")]
    pub fn to_fit(&self, time_created: u32) -> Result<Vec<u8>, zerofit::encode::EncodeError> {
        fit_workout::encode(self, time_created)
    }
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// FIT workout encoding.
///
/// The `zerofit-profile` subset covers activity files only, so the few
/// profile numbers a workout needs are written here by hand, from the
/// FIT SDK profile (21.171) `Messages` and `Types` sheets:
///
/// | Message (global) | Field | Number | Base type | Value written |
/// |---|---|---|---|---|
/// | `file_id` (0) | `type` | 0 | enum | 5 = workout |
/// | | `manufacturer` | 1 | uint16 | 255 = development |
/// | | `product` | 2 | uint16 | 0 |
/// | | `time_created` | 4 | uint32 | caller's time |
/// | `workout` (26) | `sport` | 4 | enum | 2 = cycling |
/// | | `num_valid_steps` | 6 | uint16 | number of steps |
/// | | `wkt_name` | 8 | string | name, ≤ 31 bytes |
/// | `workout_step` (27) | `message_index` | 254 | uint16 | step index |
/// | | `duration_type` | 1 | enum | 0 = time |
/// | | `duration_value` | 2 | uint32 | ms (`duration_time`, scale 1000) |
/// | | `target_type` | 3 | enum | 4 = power |
/// | | `target_value` | 4 | uint32 | 0 = custom range |
/// | | `custom_target_value_low` | 5 | uint32 | % FTP (`workout_power`: 0–1000 = %, > 1000 = W + 1000) |
/// | | `custom_target_value_high` | 6 | uint32 | % FTP |
/// | | `intensity` | 7 | enum | 0 active, 1 rest, 2 warm-up, 3 cool-down |
///
/// Devices show a power *range*. A steady step becomes a 5-point window
/// centred on its target (e.g. 103–107 % for 105 %). A ramp becomes the
/// range between its end points, because the classic `workout_step` has
/// no ramp target; the `.zwo` export keeps the ramp.
#[cfg(feature = "fit")]
pub mod fit_workout {
    use alloc::vec::Vec;

    use zerofit::encode::{EncodeError, Encoder, FileOptions};
    use zerofit::{BaseType, Endian, FieldDefinition};

    use super::{StepKind, StepTarget, Workout};
    use crate::num::round_u16;

    /// Global message number of `file_id`.
    pub const FILE_ID: u16 = 0;
    /// Global message number of `workout`.
    pub const WORKOUT: u16 = 26;
    /// Global message number of `workout_step`.
    pub const WORKOUT_STEP: u16 = 27;
    /// `file` enum value for a workout file.
    pub const FILE_TYPE_WORKOUT: u8 = 5;
    /// Longest workout name written, bytes (many devices show 15–31).
    pub const MAX_NAME_BYTES: usize = 31;

    /// Profile version written in the header: 21.171.
    const PROFILE_VERSION: u16 = 21171;

    fn field(number: u8, size: u8, base: BaseType) -> FieldDefinition {
        FieldDefinition::new(number, size, base.to_byte())
    }

    /// The `(low, high)` % FTP range of a step's target.
    #[must_use]
    pub fn power_range(target: StepTarget) -> (u16, u16) {
        match target {
            StepTarget::Steady { pct_ftp } => (round_u16(pct_ftp - 2.5), round_u16(pct_ftp + 2.5)),
            StepTarget::Ramp { from_pct, to_pct } => {
                let (a, b) = (round_u16(from_pct), round_u16(to_pct));
                (a.min(b), a.max(b))
            }
        }
    }

    const fn intensity(kind: StepKind) -> u8 {
        match kind {
            StepKind::Active => 0,
            StepKind::Recovery => 1,
            StepKind::Warmup => 2,
            StepKind::Cooldown => 3,
        }
    }

    /// Encodes `workout` (see the [module docs](self)).
    ///
    /// # Errors
    ///
    /// An encoder error (not expected for any input; see
    /// [`Workout::to_fit`]).
    pub fn encode(workout: &Workout, time_created: u32) -> Result<Vec<u8>, EncodeError> {
        let mut enc = Encoder::new();
        enc.begin_file(FileOptions::new(PROFILE_VERSION))?;

        enc.write_definition(
            0,
            Endian::Little,
            FILE_ID,
            &[
                field(0, 1, BaseType::Enum),
                field(1, 2, BaseType::UInt16),
                field(2, 2, BaseType::UInt16),
                field(4, 4, BaseType::UInt32),
            ],
            &[],
        )?;
        let mut data = Vec::with_capacity(9);
        data.push(FILE_TYPE_WORKOUT);
        data.extend_from_slice(&255u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&time_created.to_le_bytes());
        enc.write_data(0, &data)?;

        // Name: truncated on a character boundary, NUL-terminated.
        let mut end = workout.name.len().min(MAX_NAME_BYTES);
        while !workout.name.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        let name = workout.name.get(..end).unwrap_or("").as_bytes();
        let name_size = u8::try_from(name.len().saturating_add(1)).unwrap_or(u8::MAX);
        let steps = u16::try_from(workout.steps.len()).unwrap_or(u16::MAX);
        enc.write_definition(
            1,
            Endian::Little,
            WORKOUT,
            &[
                field(4, 1, BaseType::Enum),
                field(6, 2, BaseType::UInt16),
                field(8, name_size, BaseType::String),
            ],
            &[],
        )?;
        let mut data = Vec::with_capacity(4usize.saturating_add(name.len()));
        data.push(2); // sport: cycling
        data.extend_from_slice(&steps.to_le_bytes());
        data.extend_from_slice(name);
        data.push(0);
        enc.write_data(1, &data)?;

        enc.write_definition(
            2,
            Endian::Little,
            WORKOUT_STEP,
            &[
                field(254, 2, BaseType::UInt16),
                field(1, 1, BaseType::Enum),
                field(2, 4, BaseType::UInt32),
                field(3, 1, BaseType::Enum),
                field(4, 4, BaseType::UInt32),
                field(5, 4, BaseType::UInt32),
                field(6, 4, BaseType::UInt32),
                field(7, 1, BaseType::Enum),
            ],
            &[],
        )?;
        for (index, step) in (0u16..).zip(&workout.steps) {
            let (low, high) = power_range(step.target);
            let mut data = Vec::with_capacity(21);
            data.extend_from_slice(&index.to_le_bytes());
            data.push(0); // duration_type: time
            data.extend_from_slice(&step.duration_s.saturating_mul(1000).to_le_bytes());
            data.push(4); // target_type: power
            data.extend_from_slice(&0u32.to_le_bytes()); // custom target
            data.extend_from_slice(&u32::from(low).to_le_bytes());
            data.extend_from_slice(&u32::from(high).to_le_bytes());
            data.push(intensity(step.kind));
            enc.write_data(2, &data)?;
        }
        enc.end_file()?;
        enc.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn sample() -> Workout {
        let mut steps = vec![WorkoutStep::ramp(600, 50.0, 75.0, StepKind::Warmup)];
        for _ in 0..3 {
            steps.push(WorkoutStep::steady(300, 115.0, StepKind::Active));
            steps.push(WorkoutStep::steady(300, 50.0, StepKind::Recovery));
        }
        steps.push(WorkoutStep::ramp(600, 60.0, 40.0, StepKind::Cooldown));
        Workout {
            name: "3x5 VO2".into(),
            steps,
        }
    }

    #[test]
    fn ramp_average_is_mean_of_end_points() {
        let w = Workout {
            name: String::new(),
            steps: vec![WorkoutStep::ramp(600, 50.0, 100.0, StepKind::Warmup)],
        };
        let load = w.planned_load(200.0);
        assert!((load.average_power - 150.0).abs() < 0.01);
    }

    #[test]
    fn planned_load_matches_profile() {
        let w = sample();
        assert_eq!(w.duration_s(), 600 + 6 * 300 + 600);
        let load = w.planned_load(250.0);
        let profile = w.power_profile(250.0);
        assert_eq!(profile.len(), 3000);
        assert_eq!(load.normalized_power, normalized_power(&profile));
        let if_ = load.intensity_factor.unwrap();
        let expected = 3000.0 / 3600.0 * if_ * if_ * 100.0;
        assert!((load.tss.unwrap() - expected).abs() < 1e-9);
        assert!(load.normalized_power.unwrap() > load.average_power);
    }

    #[test]
    fn degenerate_workouts() {
        let empty = Workout::default();
        let load = empty.planned_load(250.0);
        assert_eq!((load.duration_s, load.tss), (0, None));
        let short = Workout {
            name: String::new(),
            steps: vec![WorkoutStep::steady(10, 100.0, StepKind::Active)],
        };
        assert_eq!(short.planned_load(250.0).normalized_power, None);
        assert_eq!(sample().planned_load(0.0).tss, None);
        let negative = WorkoutStep::steady(1, -50.0, StepKind::Active);
        assert_eq!(negative.watts_at(0, 200.0), 0.0);
    }

    #[test]
    fn zwo_structure() {
        let xml = sample().to_zwo();
        assert!(xml.starts_with("<workout_file>"));
        assert!(xml.trim_end().ends_with("</workout_file>"));
        assert_eq!(xml.matches("<SteadyState").count(), 6);
        assert!(xml.contains(r#"<Cooldown Duration="600" PowerLow="0.600" PowerHigh="0.400"/>"#));
        let odd = Workout {
            name: "a&b\"c'\u{7}".into(),
            steps: vec![WorkoutStep::ramp(60, 80.0, 90.0, StepKind::Active)],
        };
        let xml = odd.to_zwo();
        assert!(xml.contains("<name>a&amp;b&quot;c&apos;</name>"));
        assert!(xml.contains("<Ramp Duration=\"60\""));
    }
}
