//! Reading activities from FIT files (feature `fit`).
//!
//! [`read_fit`] decodes a FIT file with `zerofit` and `zerofit-profile`
//! and extracts what the analytics need: every `record` message as a
//! [`RawRecord`], timer events, the sport, and the device's own session
//! summary (useful to cross-check, never used for computing).

use alloc::vec::Vec;

use zerofit::{Decoder, Record};
use zerofit_profile::messages::{Event, Record as RecordMsg, Session};
use zerofit_profile::types;

use crate::resample::{RawRecord, TimerEvent, TimerEventKind};

/// The sport of an activity, as far as the metrics care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Sport {
    /// Cycling (including indoor and e-bike sub-sports).
    Cycling,
    /// Running.
    Running,
    /// Anything else, or not recorded.
    Other,
}

/// Summary values the device (or the platform that exported the file)
/// wrote into the first `session` message. These are *not* used by the
/// metrics; they are a reference to compare against.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SessionSummary {
    /// `total_elapsed_time`, s.
    pub total_elapsed_time: Option<f64>,
    /// `total_timer_time`, s.
    pub total_timer_time: Option<f64>,
    /// `avg_power`, W.
    pub avg_power: Option<u16>,
    /// `normalized_power`, W.
    pub normalized_power: Option<u16>,
    /// `intensity_factor`.
    pub intensity_factor: Option<f64>,
    /// `training_stress_score`.
    pub training_stress_score: Option<f64>,
    /// `threshold_power` (the FTP the writer used), W.
    pub threshold_power: Option<u16>,
    /// `avg_heart_rate`, bpm.
    pub avg_heart_rate: Option<u8>,
    /// `total_work`, J.
    pub total_work: Option<u32>,
}

/// An activity read from a FIT file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FitActivity {
    /// Every `record` message with a timestamp, in file order.
    pub records: Vec<RawRecord>,
    /// Timer start/stop events, in file order.
    pub timer_events: Vec<TimerEvent>,
    /// `record` messages skipped because they had no timestamp.
    pub records_without_timestamp: usize,
    /// Sport from the first `session` message.
    pub sport: Option<Sport>,
    /// The first `session` message's summary values.
    pub session: Option<SessionSummary>,
}

/// Decodes `bytes` and extracts the records, timer events and session.
///
/// Speed and altitude come from `enhanced_speed` / `enhanced_altitude` when
/// present (32-bit, no overflow above 65 m/s or 12 km), otherwise from
/// `speed` / `altitude`. Record timestamps use the decoder's resolution of
/// compressed timestamp headers.
///
/// # Errors
///
/// The first decoding error (bad header, CRC mismatch, truncated file…).
///
/// ```
/// # let bytes: &[u8] = &[0x0E, 0x20, 0x54, 0x08, 0x12, 0x00, 0x00, 0x00, 0x2E, 0x46, 0x49, 0x54, 0x39, 0x04, 0x40, 0x00, 0x00, 0x14, 0x00, 0x02, 0xFD, 0x04, 0x86, 0x03, 0x01, 0x02, 0x00, 0x00, 0xCA, 0x9A, 0x3B, 0x8E, 0x20, 0xD3];
/// let activity = zerofit_analytics::fit::read_fit(bytes)?;
/// assert_eq!(activity.records.len(), 1);
/// assert_eq!(activity.records[0].heart_rate, Some(142));
/// # Ok::<(), zerofit::Error>(())
/// ```
pub fn read_fit(bytes: &[u8]) -> Result<FitActivity, zerofit::Error> {
    let mut activity = FitActivity::default();
    for item in Decoder::new(bytes) {
        let Record::Data(msg) = item? else { continue };
        if let Some(r) = RecordMsg::new(msg) {
            let Some(timestamp) = msg.timestamp() else {
                activity.records_without_timestamp =
                    activity.records_without_timestamp.saturating_add(1);
                continue;
            };
            activity.records.push(RawRecord {
                timestamp,
                power: r.power(),
                heart_rate: r.heart_rate(),
                cadence: r.cadence(),
                speed: r.enhanced_speed().or_else(|| r.speed()),
                altitude: r.enhanced_altitude().or_else(|| r.altitude()),
                distance: r.distance(),
            });
        } else if let Some(e) = Event::new(msg) {
            if let Some(event) = timer_event(&e, msg.timestamp()) {
                activity.timer_events.push(event);
            }
        } else if let Some(s) = Session::new(msg) {
            if activity.session.is_none() {
                activity.sport = Some(match s.sport() {
                    Some(types::Sport::Cycling) => Sport::Cycling,
                    Some(types::Sport::Running) => Sport::Running,
                    _ => Sport::Other,
                });
                activity.session = Some(SessionSummary {
                    total_elapsed_time: s.total_elapsed_time(),
                    total_timer_time: s.total_timer_time(),
                    avg_power: s.avg_power(),
                    normalized_power: s.normalized_power(),
                    intensity_factor: s.intensity_factor(),
                    training_stress_score: s.training_stress_score(),
                    threshold_power: s.threshold_power(),
                    avg_heart_rate: s.avg_heart_rate(),
                    total_work: s.total_work(),
                });
            }
        }
    }
    Ok(activity)
}

fn timer_event(e: &Event<'_>, timestamp: Option<u32>) -> Option<TimerEvent> {
    if e.event()? != types::Event::Timer {
        return None;
    }
    let kind = match e.event_type()? {
        types::EventType::Start => TimerEventKind::Start,
        types::EventType::Stop
        | types::EventType::StopAll
        | types::EventType::StopDisable
        | types::EventType::StopDisableAll => TimerEventKind::Stop,
        _ => return None,
    };
    Some(TimerEvent {
        timestamp: timestamp?,
        kind,
    })
}
