//! Look-ahead: raising a day's carbohydrate for the work coming next.
//!
//! A day's target from its own load ([`crate::daily`]) ignores what comes
//! next, so a rest day before a 4-hour ride got 3 g/kg. Muscle glycogen
//! for tomorrow's session is stored today: *"fuel for the work required"*
//! (Impey SG et al. *Fuel for the work required: a theoretical framework
//! for carbohydrate periodization and the glycogen threshold hypothesis.*
//! Sports Med 2018;48(5):1031–1048) means the day before a long or key
//! session is fuelled for that session, and the day or two before an
//! important long event is a carbohydrate-loading day (Thomas, Erdman &
//! Burke 2016: 10–12 g/kg/day for the 36–48 h before events over 90 min
//! of sustained or intermittent exercise; Burke et al. 2011).
//!
//! Every rule only *raises* a day; none lowers it. So a day's target
//! never drops when the days ahead get harder (a property test).

use crate::PlannedSession;
use crate::daily::MODERATE_IF;

/// How important a day's event is (A: the season goal; C: training
/// through it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Priority {
    /// The goal event: carbohydrate-load for it.
    A,
    /// Important: fuel like a long or key session.
    B,
    /// A training race: no special fuelling.
    C,
}

/// One of the next days, as far as look-ahead needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayAhead<'a> {
    /// The day's sessions, sorted by start.
    pub sessions: &'a [PlannedSession],
    /// The priority if the day is an event.
    pub priority: Option<Priority>,
}

impl DayAhead<'_> {
    /// A day without sessions or event.
    pub const REST: DayAhead<'static> = DayAhead {
        sessions: &[],
        priority: None,
    };
}

/// Why a day's target was raised.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(tag = "type", rename_all = "snake_case"))]
pub enum RaiseReason {
    /// An A-priority event over 90 min within the next two days.
    CarbLoad {
        /// Days until the event (1 = tomorrow).
        days_before: u32,
        /// Duration of the event's longest session, minutes.
        event_min: u32,
    },
    /// A very long session tomorrow.
    VeryLongSessionTomorrow {
        /// Its duration, minutes.
        minutes: u32,
    },
    /// A session over 90 min tomorrow.
    LongSessionTomorrow {
        /// Its duration, minutes.
        minutes: u32,
    },
    /// A high-intensity session tomorrow.
    KeySessionTomorrow {
        /// Its duration, minutes.
        minutes: u32,
        /// Its intensity factor.
        intensity_factor: f64,
    },
}

/// A raised daily target.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Raise {
    /// What triggered it.
    pub reason: RaiseReason,
    /// The day's own target, g/kg.
    pub from_g_per_kg: f64,
    /// The raised target, g/kg.
    pub to_g_per_kg: f64,
}

impl core::fmt::Display for Raise {
    /// Plain English, e.g. "Raised: 4 h 45 min ride tomorrow".
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let duration = |f: &mut core::fmt::Formatter<'_>, m: u32| {
            let (h, m) = (m / 60, m % 60);
            match (h, m) {
                (0, m) => write!(f, "{m} min"),
                (h, 0) => write!(f, "{h} h"),
                (h, m) => write!(f, "{h} h {m:02} min"),
            }
        };
        match self.reason {
            RaiseReason::CarbLoad {
                days_before,
                event_min,
            } => {
                write!(f, "Carb-loading: ")?;
                duration(f, event_min)?;
                if days_before <= 1 {
                    write!(f, " A event tomorrow")
                } else {
                    write!(f, " A event in {days_before} days")
                }
            }
            RaiseReason::VeryLongSessionTomorrow { minutes }
            | RaiseReason::LongSessionTomorrow { minutes } => {
                write!(f, "Raised: ")?;
                duration(f, minutes)?;
                write!(f, " ride tomorrow")
            }
            RaiseReason::KeySessionTomorrow {
                minutes,
                intensity_factor,
            } => {
                write!(f, "Raised: hard ")?;
                duration(f, minutes)?;
                write!(f, " session tomorrow (IF {intensity_factor:.2})")
            }
        }
    }
}

/// Sessions longer than this (minutes) tomorrow raise today.
pub const LONG_SESSION_MIN: u32 = 90;

/// Sessions this long (minutes) or longer tomorrow are "very long".
pub const VERY_LONG_SESSION_MIN: u32 = 240;

/// Intensity factor from which a session of at least
/// [`KEY_SESSION_MIN_DURATION`] minutes is a key session.
pub const KEY_SESSION_IF: f64 = 0.80;

/// Minimum duration (minutes) of a key session.
pub const KEY_SESSION_MIN_DURATION: u32 = 45;

/// The day's target after look-ahead: `own` raised by the first rule
/// that gives the most.
///
/// | rule | today at least (g/kg) | source |
/// |---|---|---|
/// | A event > 90 min in 1–2 days | 10 at 90 min → 12 at ≥ 4.5 h | ACSM 2016 carbohydrate loading, 36–48 h |
/// | session ≥ 4 h tomorrow | 8 | the same guidance at a training-day level |
/// | session > 90 min tomorrow | 5 at 90 min → 7 at 4 h | Impey et al. 2018 |
/// | IF ≥ 0.80 for ≥ 45 min tomorrow | 5.5 | Impey et al. 2018 |
///
/// Judgment calls: the thresholds and floors. B- and C-priority events
/// count as long or key sessions by their own duration and intensity;
/// only an A event is carbohydrate-loaded for. 8 g/kg before a very long training ride (rather than the
/// full 10–12) is the bottom of the consensus "very high" range: enough
/// to start full, without the gastrointestinal cost of a full load before
/// every long weekend ride.
///
/// ```
/// use zerofit_fueling::PlannedSession;
/// use zerofit_fueling::lookahead::{DayAhead, Priority, RaiseReason, raise};
/// let saturday = [PlannedSession::new(9 * 60, 285, 0.70)];
/// let friday = raise(3.0, &[DayAhead { sessions: &saturday, priority: None }]).unwrap();
/// assert_eq!(friday.to_g_per_kg, 8.0);
/// assert_eq!(friday.to_string(), "Raised: 4 h 45 min ride tomorrow");
/// let race = [PlannedSession::new(8 * 60, 180, 0.8)];
/// let load = raise(5.0, &[DayAhead::REST, DayAhead { sessions: &race, priority: Some(Priority::A) }]).unwrap();
/// assert!(matches!(load.reason, RaiseReason::CarbLoad { days_before: 2, .. }));
/// assert!(load.to_g_per_kg >= 10.0);
/// assert!(raise(6.0, &[DayAhead::REST]).is_none());
/// ```
#[must_use]
pub fn raise(own: f64, ahead: &[DayAhead<'_>]) -> Option<Raise> {
    let mut best: Option<(f64, RaiseReason)> = None;
    let mut consider = |g: f64, reason: RaiseReason| {
        if best.is_none_or(|(b, _)| g > b) {
            best = Some((g, reason));
        }
    };
    for (i, day) in ahead.iter().take(2).enumerate() {
        let days_before = u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1);
        let longest = day
            .sessions
            .iter()
            .map(|s| s.duration_min)
            .max()
            .unwrap_or(0);
        if day.priority == Some(Priority::A) && longest > LONG_SESSION_MIN {
            let s = (f64::from(longest.saturating_sub(LONG_SESSION_MIN)) / 180.0).clamp(0.0, 1.0);
            consider(
                10.0 + 2.0 * s,
                RaiseReason::CarbLoad {
                    days_before,
                    event_min: longest,
                },
            );
        }
        if i > 0 {
            continue;
        }
        if longest >= VERY_LONG_SESSION_MIN {
            consider(
                8.0,
                RaiseReason::VeryLongSessionTomorrow { minutes: longest },
            );
        } else if longest > LONG_SESSION_MIN {
            let s = f64::from(longest.saturating_sub(LONG_SESSION_MIN)) / 150.0;
            consider(
                5.0 + 2.0 * s.clamp(0.0, 1.0),
                RaiseReason::LongSessionTomorrow { minutes: longest },
            );
        }
        if let Some(key) = day
            .sessions
            .iter()
            .filter(|s| {
                s.duration_min >= KEY_SESSION_MIN_DURATION && s.intensity_factor >= KEY_SESSION_IF
            })
            .max_by(|a, b| a.intensity_factor.total_cmp(&b.intensity_factor))
        {
            consider(
                5.5,
                RaiseReason::KeySessionTomorrow {
                    minutes: key.duration_min,
                    intensity_factor: key.intensity_factor,
                },
            );
        }
    }
    let (to, reason) = best?;
    (to > own + 1e-9).then_some(Raise {
        reason,
        from_g_per_kg: own,
        to_g_per_kg: to,
    })
}

/// Duration-weighted mean intensity factor of `sessions`, if any.
#[must_use]
pub fn mean_intensity(sessions: &[PlannedSession]) -> Option<f64> {
    let minutes: f64 = sessions.iter().map(|s| f64::from(s.duration_min)).sum();
    (minutes > 0.0).then(|| {
        sessions
            .iter()
            .map(|s| {
                let i = if s.intensity_factor.is_nan() {
                    MODERATE_IF
                } else {
                    s.intensity_factor
                };
                f64::from(s.duration_min) * i
            })
            .sum::<f64>()
            / minutes
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn day(sessions: &[PlannedSession]) -> DayAhead<'_> {
        DayAhead {
            sessions,
            priority: None,
        }
    }

    #[test]
    fn rules() {
        let s = |min, i| [PlannedSession::new(600, min, i)];
        assert!(raise(3.0, &[]).is_none());
        assert!(raise(3.0, &[DayAhead::REST]).is_none());
        assert!(raise(3.0, &[day(&s(90, 0.7))]).is_none());
        let long = raise(3.0, &[day(&s(91, 0.7))]).unwrap();
        assert!((long.to_g_per_kg - 5.0).abs() < 0.02);
        assert_eq!(raise(3.0, &[day(&s(240, 0.7))]).unwrap().to_g_per_kg, 8.0);
        assert_eq!(
            raise(3.0, &[day(&s(239, 0.7))]).unwrap().to_g_per_kg,
            7.0 - 2.0 / 150.0
        );
        let key = raise(3.0, &[day(&s(60, 0.85))]).unwrap();
        assert_eq!(key.to_g_per_kg, 5.5);
        assert_eq!(
            key.to_string(),
            "Raised: hard 1 h session tomorrow (IF 0.85)"
        );
        assert!(raise(3.0, &[day(&s(44, 1.0))]).is_none());
        // Never lowers.
        assert!(raise(9.0, &[day(&s(300, 0.7))]).is_none());
    }

    #[test]
    fn carb_loading() {
        let race = [PlannedSession::new(480, 330, 0.75)];
        let a = DayAhead {
            sessions: &race,
            priority: Some(Priority::A),
        };
        let r = raise(5.0, &[a]).unwrap();
        assert_eq!(r.to_g_per_kg, 12.0);
        assert_eq!(r.to_string(), "Carb-loading: 5 h 30 min A event tomorrow");
        let r = raise(5.0, &[DayAhead::REST, a]).unwrap();
        assert_eq!(r.to_string(), "Carb-loading: 5 h 30 min A event in 2 days");
        // Three days out: no.
        assert!(raise(5.0, &[DayAhead::REST, DayAhead::REST, a]).is_none());
        // A short A event (a criterium) is not carb-loaded for, but is a
        // key session.
        let crit = [PlannedSession::new(600, 60, 1.05)];
        let r = raise(
            3.0,
            &[DayAhead {
                sessions: &crit,
                priority: Some(Priority::A),
            }],
        )
        .unwrap();
        assert!(matches!(r.reason, RaiseReason::KeySessionTomorrow { .. }));
        // A C race is training: the training-day rule, no loading.
        let c = DayAhead {
            sessions: &race,
            priority: Some(Priority::C),
        };
        let r = raise(3.0, &[c]).unwrap();
        assert_eq!(r.to_g_per_kg, 8.0);
        assert!(matches!(
            r.reason,
            RaiseReason::VeryLongSessionTomorrow { .. }
        ));
    }

    #[test]
    fn mean_if() {
        assert_eq!(mean_intensity(&[]), None);
        let s = [
            PlannedSession::new(0, 60, 0.6),
            PlannedSession::new(100, 30, 0.9),
        ];
        assert!((mean_intensity(&s).unwrap() - 0.7).abs() < 1e-12);
    }
}
