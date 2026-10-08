//! Daily carbohydrate and protein targets from the day's training load.

use crate::{Athlete, PlannedSession};

/// Intensity factor treated as "moderate" (weight 1.0) in the load index.
pub const MODERATE_IF: f64 = 0.75;

/// Power-to-weight assumed for estimating work when a session has neither
/// planned kJ nor an athlete FTP: 3.0 W/kg at FTP, a trained recreational
/// cyclist. Only used as a last resort; pass `work_kj` or an FTP instead.
pub const FALLBACK_FTP_W_PER_KG: f64 = 3.0;

/// How strongly intensity raises carbohydrate need per kJ:
/// `clamp(IF / 0.75, 0.6, 1.4)`.
///
/// The share of energy that comes from carbohydrate rises with intensity:
/// roughly half at ~55 % of `VO2max` and most of it above ~75 % (Romijn et al.
/// 1993; van Loon et al. 2001). So a kJ ridden hard costs more glycogen
/// than a kJ ridden easy. The weight is linear in IF around a moderate
/// IF of 0.75 and clamped so a recovery spin or a sprint session can't
/// dominate the day.
///
/// ```
/// use zerofit_fueling::daily::intensity_weight;
/// assert_eq!(intensity_weight(0.75), 1.0);
/// assert_eq!(intensity_weight(0.3), 0.6);
/// assert_eq!(intensity_weight(1.2), 1.4);
/// ```
#[must_use]
pub fn intensity_weight(intensity_factor: f64) -> f64 {
    if intensity_factor.is_nan() {
        return 1.0;
    }
    (intensity_factor / MODERATE_IF).clamp(0.6, 1.4)
}

/// Mechanical work of a session, kJ: the planned `work_kj` if given, else
/// `duration × IF × FTP`, with FTP from the athlete or
/// [`FALLBACK_FTP_W_PER_KG`] × body mass.
///
/// ```
/// use zerofit_fueling::{Athlete, PlannedSession, daily::session_work_kj};
/// let athlete = Athlete { body_mass_kg: 70.0, ftp_w: Some(250.0) };
/// let one_hour = PlannedSession::new(6 * 60, 60, 0.8);
/// assert_eq!(session_work_kj(&one_hour, &athlete), 720.0); // 3600 s × 200 W
/// ```
#[must_use]
pub fn session_work_kj(session: &PlannedSession, athlete: &Athlete) -> f64 {
    if let Some(kj) = session.work_kj.filter(|k| k.is_finite() && *k >= 0.0) {
        return kj;
    }
    let ftp = athlete
        .ftp_w
        .filter(|f| f.is_finite() && *f > 0.0)
        .unwrap_or(FALLBACK_FTP_W_PER_KG * athlete.body_mass_kg);
    let seconds = f64::from(session.duration_min) * 60.0;
    seconds * session.intensity_factor.max(0.0) * ftp / 1000.0
}

/// A session's effective load, kJ/kg: work per kg of body mass times
/// [`intensity_weight`].
///
/// Work per kg is used rather than hours alone because carbohydrate
/// oxidation scales with energy expended, and per kg because every
/// consensus target is expressed per kg. One hour at a moderate intensity
/// for a 3.5 W/kg rider is about 9 kJ/kg.
#[must_use]
pub fn session_load(session: &PlannedSession, athlete: &Athlete) -> f64 {
    if athlete.body_mass_kg <= 0.0 {
        return 0.0;
    }
    session_work_kj(session, athlete) / athlete.body_mass_kg
        * intensity_weight(session.intensity_factor)
}

/// The day's effective load, kJ/kg: the sum of [`session_load`].
#[must_use]
pub fn day_load(sessions: &[PlannedSession], athlete: &Athlete) -> f64 {
    sessions.iter().map(|s| session_load(s, athlete)).sum()
}

/// The consensus training-load categories for daily carbohydrate intake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum LoadBand {
    /// Low intensity or skill-based activity: 3–5 g/kg/day.
    Light,
    /// Moderate exercise (~1 h/day): 5–7 g/kg/day.
    Moderate,
    /// Endurance programme, 1–3 h/day moderate-to-high intensity:
    /// 6–10 g/kg/day.
    High,
    /// Extreme commitment, 4–5+ h/day moderate-to-high intensity:
    /// 8–12 g/kg/day.
    VeryHigh,
}

impl LoadBand {
    /// The consensus range for this band, g/kg/day (Thomas, Erdman & Burke
    /// 2016, Table 2; Burke et al. 2011).
    #[must_use]
    pub const fn range_g_per_kg(self) -> (f64, f64) {
        match self {
            Self::Light => (3.0, 5.0),
            Self::Moderate => (5.0, 7.0),
            Self::High => (6.0, 10.0),
            Self::VeryHigh => (8.0, 12.0),
        }
    }

    /// The band for an effective day load `x` (kJ/kg): Light below 8,
    /// Moderate below 15, High below 35, Very high from 35.
    ///
    /// ```
    /// use zerofit_fueling::daily::LoadBand;
    /// assert_eq!(LoadBand::for_load(0.0), LoadBand::Light);
    /// assert_eq!(LoadBand::for_load(9.0), LoadBand::Moderate);
    /// assert_eq!(LoadBand::for_load(20.0), LoadBand::High);
    /// assert_eq!(LoadBand::for_load(40.0), LoadBand::VeryHigh);
    /// ```
    #[must_use]
    pub fn for_load(x: f64) -> Self {
        if x < LIGHT_MAX {
            Self::Light
        } else if x < MODERATE_MAX {
            Self::Moderate
        } else if x < HIGH_MAX {
            Self::High
        } else {
            Self::VeryHigh
        }
    }
}

const LIGHT_MAX: f64 = 8.0;
const MODERATE_MAX: f64 = 15.0;
const HIGH_MAX: f64 = 35.0;

/// `(load kJ/kg, carbohydrate g/kg/day)` anchors of [`daily_carbs_g_per_kg`].
pub const CARB_ANCHORS: [(f64, f64); 5] = [
    (0.0, 3.0),
    (LIGHT_MAX, 5.0),
    (MODERATE_MAX, 6.5),
    (HIGH_MAX, 9.0),
    (60.0, 12.0),
];

/// Daily carbohydrate target, g/kg body mass, for an effective day load
/// `x` (kJ/kg, see [`day_load`]).
///
/// The consensus (Thomas, Erdman & Burke 2016, ACSM/AND/DC position
/// statement; Burke et al. 2011, *J Sports Sci*; IOC consensus 2011) gives
/// ranges per load category: light 3–5, moderate (~1 h/day) 5–7, high
/// (1–3 h/day) 6–10 and very high (4–5+ h/day) 8–12 g/kg/day. Instead of
/// jumping between buckets, the target is piecewise linear in the load,
/// with anchors at the band boundaries ([`CARB_ANCHORS`]):
///
/// | load x (kJ/kg) | 0 | 8 | 15 | 35 | ≥ 60 |
/// |---|---|---|---|---|---|
/// | g/kg/day | 3.0 | 5.0 | 6.5 | 9.0 | 12.0 |
///
/// Every target therefore lies inside its band's consensus range, nearby
/// loads get nearby targets, and the target never falls as load rises
/// (a property test checks this).
///
/// For a 70 kg rider with FTP 250 W: 1 h at IF 0.70 is x ≈ 8.4 →
/// 5.1 g/kg (moderate); 2 h at IF 0.75 is x ≈ 19 → 7.0 g/kg (high);
/// 5 h at IF 0.65 is x ≈ 36 → 9.1 g/kg (very high).
///
/// Judgment call: the band boundaries in kJ/kg. They correspond to the
/// consensus durations at moderate intensity for a ~3.5 W/kg rider
/// (≈ 1 h, ≈ 1.7 h, ≈ 4 h); a weaker or stronger rider doing the same
/// hours does proportionally less or more work, and the targets follow
/// the work.
///
/// ```
/// use zerofit_fueling::daily::daily_carbs_g_per_kg;
/// assert_eq!(daily_carbs_g_per_kg(0.0), 3.0);
/// assert_eq!(daily_carbs_g_per_kg(8.0), 5.0);
/// assert_eq!(daily_carbs_g_per_kg(25.0), 7.75);
/// assert_eq!(daily_carbs_g_per_kg(100.0), 12.0);
/// ```
#[must_use]
pub fn daily_carbs_g_per_kg(x: f64) -> f64 {
    interpolate(&CARB_ANCHORS, x)
}

/// Daily protein target, g/kg body mass: `1.2 + 0.8 · min(x / 60, 1)`.
///
/// The consensus range for athletes is 1.2–2.0 g/kg/day (Thomas, Erdman &
/// Burke 2016; Phillips & van Loon 2011), toward the upper end in heavy
/// training. The target rises linearly with the same load index as
/// carbohydrate and reaches 2.0 at the top of the very-high band. Spread
/// it over the day: ~0.3 g/kg per meal every 3–5 h (Moore et al. 2015;
/// Areta et al. 2013), which is how [`crate::day_plan`] distributes it.
///
/// ```
/// use zerofit_fueling::daily::daily_protein_g_per_kg;
/// assert_eq!(daily_protein_g_per_kg(0.0), 1.2);
/// assert_eq!(daily_protein_g_per_kg(30.0), 1.6);
/// assert_eq!(daily_protein_g_per_kg(90.0), 2.0);
/// ```
#[must_use]
pub fn daily_protein_g_per_kg(x: f64) -> f64 {
    if x.is_nan() {
        return 1.2;
    }
    1.2 + 0.8 * (x / 60.0).clamp(0.0, 1.0)
}

/// Piecewise-linear interpolation, flat outside the anchors. NaN maps to
/// the first anchor.
fn interpolate(anchors: &[(f64, f64)], x: f64) -> f64 {
    let (Some(&(x0, y0)), Some(&(_, y_last))) = (anchors.first(), anchors.last()) else {
        return 0.0;
    };
    if x.is_nan() || x <= x0 {
        return y0;
    }
    for pair in anchors.windows(2) {
        if let [(xa, ya), (xb, yb)] = *pair {
            if x <= xb {
                return ya + (yb - ya) * (x - xa) / (xb - xa);
            }
        }
    }
    y_last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn anchors_are_exact() {
        for (x, y) in CARB_ANCHORS {
            assert!(close(daily_carbs_g_per_kg(x), y), "{x}");
        }
        assert_eq!(daily_carbs_g_per_kg(-5.0), 3.0);
        assert_eq!(daily_carbs_g_per_kg(f64::NAN), 3.0);
        assert_eq!(daily_carbs_g_per_kg(f64::INFINITY), 12.0);
    }

    #[test]
    fn every_target_is_inside_its_band() {
        let mut x = 0.0;
        while x < 80.0 {
            let band = LoadBand::for_load(x);
            let (lo, hi) = band.range_g_per_kg();
            let g = daily_carbs_g_per_kg(x);
            assert!(
                g >= lo - 1e-9 && g <= hi + 1e-9,
                "x {x}: {g} not in {band:?}"
            );
            x += 0.25;
        }
    }

    #[test]
    fn band_boundaries() {
        assert_eq!(LoadBand::for_load(7.999), LoadBand::Light);
        assert_eq!(LoadBand::for_load(8.0), LoadBand::Moderate);
        assert_eq!(LoadBand::for_load(14.999), LoadBand::Moderate);
        assert_eq!(LoadBand::for_load(15.0), LoadBand::High);
        assert_eq!(LoadBand::for_load(34.999), LoadBand::High);
        assert_eq!(LoadBand::for_load(35.0), LoadBand::VeryHigh);
    }

    #[test]
    fn worked_examples_from_the_docs() {
        let athlete = Athlete {
            body_mass_kg: 70.0,
            ftp_w: Some(250.0),
        };
        let day = |minutes, intensity| {
            day_load(&[PlannedSession::new(360, minutes, intensity)], &athlete)
        };
        let moderate = day(60, 0.70);
        assert_eq!(LoadBand::for_load(moderate), LoadBand::Moderate);
        assert!((daily_carbs_g_per_kg(moderate) - 5.1).abs() < 0.05);
        let high = day(120, 0.75);
        assert_eq!(LoadBand::for_load(high), LoadBand::High);
        assert!((daily_carbs_g_per_kg(high) - 7.0).abs() < 0.05);
        let very_high = day(300, 0.65);
        assert_eq!(LoadBand::for_load(very_high), LoadBand::VeryHigh);
        assert!((daily_carbs_g_per_kg(very_high) - 9.1).abs() < 0.05);
        assert_eq!(LoadBand::for_load(day(30, 0.6)), LoadBand::Light);
    }

    #[test]
    fn work_estimates() {
        let athlete = Athlete {
            body_mass_kg: 80.0,
            ftp_w: None,
        };
        // Fallback 3.0 W/kg: 240 W FTP; 1 h at IF 1.0 is 864 kJ.
        let s = PlannedSession::new(0, 60, 1.0);
        assert!(close(session_work_kj(&s, &athlete), 864.0));
        let planned = PlannedSession {
            work_kj: Some(500.0),
            ..s
        };
        assert_eq!(session_work_kj(&planned, &athlete), 500.0);
        let zero_mass = Athlete {
            body_mass_kg: 0.0,
            ftp_w: None,
        };
        assert_eq!(session_load(&s, &zero_mass), 0.0);
    }

    #[test]
    fn protein_range() {
        assert_eq!(daily_protein_g_per_kg(-1.0), 1.2);
        assert_eq!(daily_protein_g_per_kg(f64::NAN), 1.2);
        assert_eq!(daily_protein_g_per_kg(60.0), 2.0);
    }

    #[test]
    fn intensity_weight_nan_is_neutral() {
        assert_eq!(intensity_weight(f64::NAN), 1.0);
    }
}
