//! Athlete settings: thresholds, body weight and zone definitions.

use alloc::vec::Vec;

/// Which set of Banister TRIMP coefficients to use for hrTSS.
///
/// Banister's TRIMP weights each minute by `a·e^(b·x)`, where `x` is the
/// heart-rate-reserve fraction. The coefficients were fitted separately to
/// the blood lactate response of men (`a = 0.64`, `b = 1.92`) and women
/// (`a = 0.86`, `b = 1.67`) (Banister 1991; Morton, Fitz-Clarke & Banister
/// 1990).
///
/// ```
/// use zerofit_analytics::TrimpCoefficients;
/// let (a, b) = TrimpCoefficients::Female.coefficients();
/// assert_eq!((a, b), (0.86, 1.67));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum TrimpCoefficients {
    /// `0.64·e^(1.92x)`, the default (Banister's original constants).
    #[default]
    Male,
    /// `0.86·e^(1.67x)`.
    Female,
}

impl TrimpCoefficients {
    /// `(a, b)` in `a·e^(b·x)`.
    #[must_use]
    pub const fn coefficients(self) -> (f64, f64) {
        match self {
            Self::Male => (0.64, 1.92),
            Self::Female => (0.86, 1.67),
        }
    }
}

/// Zone boundaries as fractions of a reference value (FTP for power, LTHR
/// for heart rate).
///
/// `upper` holds the *inclusive* upper bound of every zone but the last, in
/// increasing order, so `n` bounds define `n + 1` zones. A value `v` is in
/// zone `i` (0-based) if `v ≤ upper[i]·reference` and it is above the bound
/// of the zone below. Inclusive upper bounds match how Coggan's and Friel's
/// tables are written ("Z2: 56–75 %"): exactly 75 % of FTP is Z2.
///
/// ```
/// use zerofit_analytics::Zones;
/// let zones = Zones::coggan_power();
/// assert_eq!(zones.len(), 7);
/// assert_eq!(zones.zone_of(250.0, 250.0), 3); // 100 % FTP: Z4
/// assert_eq!(zones.zone_of(187.5, 250.0), 1); // exactly 75 %: Z2
/// assert_eq!(zones.zone_of(400.0, 250.0), 6); // 160 %: Z7
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Zones {
    upper: Vec<f64>,
}

impl Zones {
    /// Zones from inclusive upper bounds (fractions of the reference).
    ///
    /// Returns `None` unless the bounds are finite, positive and strictly
    /// increasing.
    ///
    /// ```
    /// use zerofit_analytics::Zones;
    /// assert!(Zones::new(vec![0.6, 0.8, 1.0]).is_some());
    /// assert!(Zones::new(vec![0.8, 0.6]).is_none());
    /// ```
    #[must_use]
    pub fn new(upper: Vec<f64>) -> Option<Self> {
        let finite_positive = upper.iter().all(|b| b.is_finite() && *b > 0.0);
        let increasing = upper.windows(2).all(|w| matches!(w, [a, b] if a < b));
        (finite_positive && increasing).then_some(Self { upper })
    }

    /// Coggan's seven power levels, as fractions of FTP:
    /// Z1 ≤ 55 %, Z2 ≤ 75 %, Z3 ≤ 90 %, Z4 ≤ 105 %, Z5 ≤ 120 %, Z6 ≤ 150 %,
    /// Z7 above (Allen & Coggan, *Training and Racing with a Power Meter*).
    /// These are also intervals.icu's default power zones.
    #[must_use]
    pub fn coggan_power() -> Self {
        Self {
            upper: alloc::vec![0.55, 0.75, 0.90, 1.05, 1.20, 1.50],
        }
    }

    /// Friel's seven cycling heart-rate zones, as fractions of LTHR:
    /// Z1 ≤ 81 %, Z2 ≤ 89 %, Z3 ≤ 93 %, Z4 ≤ 99 %, Z5a ≤ 102 %, Z5b ≤ 106 %,
    /// Z5c above (Friel, *The Cyclist's Training Bible*).
    #[must_use]
    pub fn friel_hr() -> Self {
        Self {
            upper: alloc::vec![0.81, 0.89, 0.93, 0.99, 1.02, 1.06],
        }
    }

    /// The inclusive upper bounds.
    #[must_use]
    pub fn upper_bounds(&self) -> &[f64] {
        &self.upper
    }

    /// Number of zones (`upper_bounds().len() + 1`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.upper.len().saturating_add(1)
    }

    /// Always `false`: there is at least one zone.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// The 0-based zone of `value` for the given `reference`.
    #[must_use]
    pub fn zone_of(&self, value: f64, reference: f64) -> usize {
        self.upper
            .iter()
            .position(|b| value <= b * reference)
            .unwrap_or(self.upper.len())
    }
}

/// Everything the metrics need to know about the athlete.
///
/// All thresholds are optional: a metric whose inputs are missing returns
/// `None` rather than guessing.
///
/// ```
/// use zerofit_analytics::AthleteSettings;
/// let athlete = AthleteSettings {
///     ftp: Some(250.0),
///     weight_kg: Some(72.0),
///     lthr: Some(165),
///     max_hr: Some(185),
///     resting_hr: Some(50),
///     ..AthleteSettings::default()
/// };
/// assert_eq!(athlete.cp_or_ftp(), Some(250.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct AthleteSettings {
    /// Functional threshold power, W. Used for IF, TSS and power zones.
    pub ftp: Option<f64>,
    /// Body weight, kg. Used for W/kg.
    pub weight_kg: Option<f64>,
    /// Lactate threshold heart rate, bpm. Used for hrTSS and HR zones.
    pub lthr: Option<u8>,
    /// Maximum heart rate, bpm. Used for hrTSS (heart rate reserve).
    pub max_hr: Option<u8>,
    /// Resting heart rate, bpm. Used for hrTSS (heart rate reserve).
    pub resting_hr: Option<u8>,
    /// TRIMP coefficients for hrTSS.
    pub trimp: TrimpCoefficients,
    /// Critical power, W, for W'bal. Falls back to FTP if `None`.
    pub cp: Option<f64>,
    /// W' (anaerobic work capacity), J, for W'bal. Falls back to
    /// [`DEFAULT_W_PRIME`] if `None`.
    pub w_prime: Option<f64>,
    /// Power zones, relative to FTP.
    pub power_zones: Zones,
    /// Heart-rate zones, relative to LTHR.
    pub hr_zones: Zones,
}

/// W' used for W'bal when the athlete has none set: 20 kJ, a typical value
/// for a trained cyclist (Skiba 2012 reports 15–25 kJ).
pub const DEFAULT_W_PRIME: f64 = 20_000.0;

impl Default for AthleteSettings {
    fn default() -> Self {
        Self {
            ftp: None,
            weight_kg: None,
            lthr: None,
            max_hr: None,
            resting_hr: None,
            trimp: TrimpCoefficients::default(),
            cp: None,
            w_prime: None,
            power_zones: Zones::coggan_power(),
            hr_zones: Zones::friel_hr(),
        }
    }
}

impl AthleteSettings {
    /// Critical power for W'bal: `cp`, else `ftp`.
    ///
    /// Judgment call: FTP and CP are different quantities (CP is typically
    /// a few percent above a 1-hour FTP), but FTP is what most athletes
    /// have set, and intervals.icu also defaults W'bal to the athlete's FTP.
    #[must_use]
    pub fn cp_or_ftp(&self) -> Option<f64> {
        self.cp.or(self.ftp).filter(|p| *p > 0.0)
    }

    /// W' for W'bal: `w_prime`, else [`DEFAULT_W_PRIME`].
    #[must_use]
    pub fn w_prime_or_default(&self) -> f64 {
        self.w_prime.filter(|w| *w > 0.0).unwrap_or(DEFAULT_W_PRIME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_edges_are_inclusive() {
        let z = Zones::coggan_power();
        assert_eq!(z.zone_of(0.0, 200.0), 0);
        assert_eq!(z.zone_of(110.0, 200.0), 0); // exactly 55 %
        assert_eq!(z.zone_of(110.1, 200.0), 1);
        assert_eq!(z.zone_of(300.0, 200.0), 5); // exactly 150 %
        assert_eq!(z.zone_of(300.1, 200.0), 6);
    }

    #[test]
    fn rejects_bad_bounds() {
        assert!(Zones::new(alloc::vec![f64::NAN]).is_none());
        assert!(Zones::new(alloc::vec![0.0, 1.0]).is_none());
        assert!(Zones::new(alloc::vec![1.0, 1.0]).is_none());
        assert_eq!(Zones::new(alloc::vec![]).map(|z| z.len()), Some(1));
    }

    #[test]
    fn fallbacks() {
        let a = AthleteSettings::default();
        assert_eq!(a.cp_or_ftp(), None);
        assert_eq!(a.w_prime_or_default(), DEFAULT_W_PRIME);
        let a = AthleteSettings {
            ftp: Some(250.0),
            cp: Some(260.0),
            ..AthleteSettings::default()
        };
        assert_eq!(a.cp_or_ftp(), Some(260.0));
    }
}
