//! Raw records → a 1 Hz [`ActivityStream`].
//!
//! Every metric in this crate is a sum or a rolling window over one sample
//! per second, so how the raw records become that series decides the
//! answer. A 30 s café stop counted as 30 zeros lowers average power and
//! NP; interpolating across it inflates them; a duplicated timestamp
//! counted twice adds a second of work that never happened. This module
//! makes each of those decisions explicitly and reports how often each
//! one applied ([`ResampleReport`]).
//!
//! # Rules
//!
//! The defaults follow intervals.icu, which builds a 1 s interpolated
//! series and repairs short power dropouts before computing anything
//! (developer posts on forum.intervals.icu: "`fixed_watts` … short drop outs
//! (8s or less) fixed and power spikes removed"; recording time is "the
//! elapsed time … with gaps (not recording e.g. paused) of more than 30s
//! removed").
//!
//! | Situation | Treatment | Setting |
//! |---|---|---|
//! | Two records with the same timestamp | Merged field by field: the later record's value wins where it has one, otherwise the earlier one's is kept. One second of data. | — |
//! | A timestamp earlier than the previous record's | Dropped (a clock correction or a corrupt record; re-sorting could fabricate a ride). | — |
//! | No records for more than 30 s (auto-pause, manual pause, or the device stopped recording) | A **pause**: the missing seconds are removed. The stream is recording time; elapsed time is kept per sample. | [`pause_gap`](ResampleConfig::pause_gap) |
//! | No records for 1–30 s (smart recording, a dropped packet, a short stop with auto-pause off) | **Filled**: every channel is linearly interpolated between the records on either side, and the samples are marked [`SampleState::Filled`]. | [`pause_gap`](ResampleConfig::pause_gap) |
//! | Records present but one field missing (sensor dropout), up to 8 s | **Repaired**: linearly interpolated between the neighbouring values, or held if the dropout is at the start or end of a segment. | [`max_dropout`](ResampleConfig::max_dropout) |
//! | Field missing for more than 8 s | Left missing. Power becomes 0 W and is flagged as a dropout; HR, cadence, speed, altitude and distance stay `None` and are excluded from their averages. | [`max_dropout`](ResampleConfig::max_dropout) |
//! | Power above 2500 W | A **spike**: treated as missing, then repaired like a dropout. | [`max_power`](ResampleConfig::max_power) |
//! | Timer stop/start events | Ignored by default, because auto-pause already shows up as a gap in the records. With [`use_timer_events`](ResampleConfig::use_timer_events), records between a stop and the next start are dropped and the stop is always a pause, even if it is short. | [`use_timer_events`](ResampleConfig::use_timer_events) |
//! | Invalid values (FIT sentinels) | Already `None` from the decoder, so they are dropouts. | — |
//! | Sub-second timestamps | FIT `record` timestamps are whole seconds. Callers with finer timestamps should floor them, after which the duplicate rule applies. | — |
//!
//! Judgment calls:
//!
//! - **Zeros are kept.** Coasting records with 0 W are real data and count
//!   in every average, as in TrainingPeaks and intervals.icu. Only seconds
//!   with *no record* are removed (pauses) or filled (short gaps).
//! - **Power is interpolated across short gaps**, not zeroed: smart
//!   recording writes a record when something changes, so the true power
//!   in between is close to the line between the two records.
//! - **A filled run is bounded by the dropout limit only for seconds that
//!   had a record.** A 20 s gap with no records is filled even though it
//!   is longer than 8 s, because those seconds were never recorded; a
//!   20 s run of records *without power* is a sensor dropout and is not.
//!
//! ```
//! use zerofit_analytics::resample::{RawRecord, ResampleConfig, resample};
//!
//! let records = [
//!     RawRecord { power: Some(200), ..RawRecord::at(100) },
//!     RawRecord { power: Some(300), ..RawRecord::at(102) }, // 1 s gap: filled
//!     RawRecord { power: Some(100), ..RawRecord::at(200) }, // 97 s gap: pause
//! ];
//! let (stream, report) = resample(&records, &[], &ResampleConfig::default());
//! assert_eq!(stream.power(), &[200, 250, 300, 100]);
//! assert_eq!(stream.elapsed(), &[0, 1, 2, 100]);
//! assert_eq!((report.gap_seconds_filled, report.paused_seconds), (1, 97));
//! ```

use alloc::vec::Vec;

use crate::num::{f64_from_usize, round_u8, round_u16};
use crate::stream::{ActivityStream, Sample, SampleState};

/// One record as written by the device. Every field but the timestamp is
/// optional.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RawRecord {
    /// FIT time, s since 1989-12-31T00:00:00Z (any monotonic seconds
    /// clock works).
    pub timestamp: u32,
    /// Power, W.
    pub power: Option<u16>,
    /// Heart rate, bpm.
    pub heart_rate: Option<u8>,
    /// Cadence, rpm.
    pub cadence: Option<u8>,
    /// Speed, m/s.
    pub speed: Option<f64>,
    /// Altitude, m.
    pub altitude: Option<f64>,
    /// Distance from the start, m.
    pub distance: Option<f64>,
}

impl RawRecord {
    /// A record at `timestamp` with no values.
    ///
    /// ```
    /// use zerofit_analytics::resample::RawRecord;
    /// let r = RawRecord { heart_rate: Some(150), ..RawRecord::at(10) };
    /// assert_eq!((r.timestamp, r.power), (10, None));
    /// ```
    #[must_use]
    pub const fn at(timestamp: u32) -> Self {
        Self {
            timestamp,
            power: None,
            heart_rate: None,
            cadence: None,
            speed: None,
            altitude: None,
            distance: None,
        }
    }

    /// Fills this record's missing fields from `later`, letting `later`'s
    /// values win: the duplicate-timestamp rule.
    fn merge(&mut self, later: &Self) {
        self.power = later.power.or(self.power);
        self.heart_rate = later.heart_rate.or(self.heart_rate);
        self.cadence = later.cadence.or(self.cadence);
        self.speed = later.speed.or(self.speed);
        self.altitude = later.altitude.or(self.altitude);
        self.distance = later.distance.or(self.distance);
    }
}

/// A timer event (FIT `event` message with `event = timer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerEvent {
    /// FIT time of the event, s.
    pub timestamp: u32,
    /// Start or stop.
    pub kind: TimerEventKind,
}

/// Whether the timer started or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerEventKind {
    /// Timer started or resumed (`event_type = start`).
    Start,
    /// Timer stopped or paused (`event_type = stop`, `stop_all`, …).
    Stop,
}

/// Resampler settings. See the [module docs](self) for what each does.
///
/// ```
/// use zerofit_analytics::resample::ResampleConfig;
/// let c = ResampleConfig::default();
/// assert_eq!((c.pause_gap, c.max_dropout, c.max_power), (30, 8, Some(2500)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResampleConfig {
    /// Missing seconds between two records above which the gap is a pause
    /// (removed) rather than filled. Default 30 (intervals.icu).
    pub pause_gap: u32,
    /// Longest field dropout, in recorded seconds, that is repaired by
    /// interpolation. Default 8 (intervals.icu's `fixed_watts`).
    pub max_dropout: u32,
    /// Power above this is a spike and treated as missing. Default
    /// 2500 W; `None` keeps every value.
    pub max_power: Option<u16>,
    /// Drop records between timer stop and start events. Default `false`.
    pub use_timer_events: bool,
}

impl Default for ResampleConfig {
    fn default() -> Self {
        Self {
            pause_gap: 30,
            max_dropout: 8,
            max_power: Some(2500),
            use_timer_events: false,
        }
    }
}

/// What the resampler did, for diagnostics and reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ResampleReport {
    /// Records passed in.
    pub records: usize,
    /// Records merged into an earlier record with the same timestamp.
    pub duplicates: usize,
    /// Records dropped because their timestamp went backwards.
    pub out_of_order: usize,
    /// Records dropped because they fell inside a timer pause (only with
    /// [`ResampleConfig::use_timer_events`]).
    pub timer_paused_records: usize,
    /// Power values above [`ResampleConfig::max_power`] discarded.
    pub power_spikes: usize,
    /// Short gaps (no records) that were filled.
    pub gaps_filled: usize,
    /// Seconds added by filling short gaps.
    pub gap_seconds_filled: usize,
    /// Gaps treated as pauses.
    pub pauses: usize,
    /// Seconds removed as paused.
    pub paused_seconds: usize,
    /// Recorded seconds whose missing power was repaired by interpolation.
    pub power_dropouts_repaired: usize,
    /// Seconds whose power stayed missing and was set to 0 W (only
    /// counted when the activity has power at all).
    pub power_dropout_seconds: usize,
    /// Recorded seconds whose missing heart rate was repaired.
    pub hr_dropouts_repaired: usize,
}

/// Resamples `records` to one sample per second of recording time.
///
/// `records` should be in file order; `timer` is only used with
/// [`ResampleConfig::use_timer_events`]. The stream's start time is the
/// first kept record's timestamp. See the [module docs](self) for the
/// rules.
///
/// Runs in O(n + s) time, where s is the number of output seconds.
///
/// ```
/// use zerofit_analytics::resample::{RawRecord, ResampleConfig, resample};
/// let records = [
///     RawRecord { power: Some(100), ..RawRecord::at(0) },
///     RawRecord { power: Some(999), ..RawRecord::at(0) }, // duplicate: later wins
/// ];
/// let (stream, report) = resample(&records, &[], &ResampleConfig::default());
/// assert_eq!(stream.power(), &[999]);
/// assert_eq!(report.duplicates, 1);
/// ```
#[must_use]
pub fn resample(
    records: &[RawRecord],
    timer: &[TimerEvent],
    config: &ResampleConfig,
) -> (ActivityStream, ResampleReport) {
    let mut report = ResampleReport {
        records: records.len(),
        ..ResampleReport::default()
    };
    let pauses = if config.use_timer_events {
        timer_pauses(timer)
    } else {
        Vec::new()
    };
    let merged = merge_records(records, &pauses, config, &mut report);
    let Some(first) = merged.first() else {
        return (ActivityStream::new(0), report);
    };
    let start = first.timestamp;

    // One slot per output second: the record it came from (if any) and
    // the segment (run between pauses) it belongs to.
    let mut slots: Vec<Slot> = Vec::with_capacity(merged.len());
    let mut segment = 0u32;
    let mut previous: Option<u32> = None;
    for (index, record) in merged.iter().enumerate() {
        if let Some(prev) = previous {
            let missing = record.timestamp.wrapping_sub(prev).saturating_sub(1);
            let timer_break = pauses
                .iter()
                .any(|p| p.stop > prev && p.stop <= record.timestamp);
            if missing > config.pause_gap || timer_break {
                segment = segment.saturating_add(1);
                report.pauses = report.pauses.saturating_add(1);
                report.paused_seconds = report
                    .paused_seconds
                    .saturating_add(usize::try_from(missing).unwrap_or(usize::MAX));
            } else if missing > 0 {
                report.gaps_filled = report.gaps_filled.saturating_add(1);
                for t in prev.saturating_add(1)..record.timestamp {
                    slots.push(Slot {
                        elapsed: t.wrapping_sub(start),
                        record: None,
                        segment,
                    });
                    report.gap_seconds_filled = report.gap_seconds_filled.saturating_add(1);
                }
            }
        }
        slots.push(Slot {
            elapsed: record.timestamp.wrapping_sub(start),
            record: Some(index),
            segment,
        });
        previous = Some(record.timestamp);
    }

    let field = |f: fn(&RawRecord) -> Option<f64>| -> Vec<Option<f64>> {
        slots
            .iter()
            .map(|s| s.record.and_then(|i| merged.get(i)).and_then(f))
            .collect()
    };
    let recorded: Vec<bool> = slots.iter().map(|s| s.record.is_some()).collect();
    let segments = segment_ranges(&slots);
    let max_dropout = usize::try_from(config.max_dropout).unwrap_or(usize::MAX);
    let fill = |mut values: Vec<Option<f64>>| -> (Vec<Option<f64>>, usize) {
        let repaired = fill_channel(&mut values, &recorded, &segments, max_dropout);
        (values, repaired)
    };

    let (power, power_repaired) = fill(field(|r| r.power.map(f64::from)));
    let (heart_rate, hr_repaired) = fill(field(|r| r.heart_rate.map(f64::from)));
    let (cadence, _) = fill(field(|r| r.cadence.map(f64::from)));
    let (speed, _) = fill(field(|r| r.speed));
    let (altitude, _) = fill(field(|r| r.altitude));
    let (distance, _) = fill(field(|r| r.distance));
    report.power_dropouts_repaired = power_repaired;
    report.hr_dropouts_repaired = hr_repaired;

    let mut stream = ActivityStream::new(start);
    stream.reserve(slots.len());
    let channels = power
        .iter()
        .zip(&heart_rate)
        .zip(&cadence)
        .zip(&speed)
        .zip(&altitude)
        .zip(&distance);
    for (slot, (((((p, hr), cad), spd), alt), dist)) in slots.iter().zip(channels) {
        let sample = Sample {
            elapsed: slot.elapsed,
            power: p.map(round_u16),
            heart_rate: hr.map(round_u8),
            cadence: cad.map(round_u8),
            speed: *spd,
            altitude: *alt,
            distance: *dist,
            state: if slot.record.is_some() {
                SampleState::Recorded
            } else {
                SampleState::Filled
            },
        };
        // `elapsed` is strictly increasing by construction, so this
        // cannot fail; a rejected sample would only be skipped.
        let _ = stream.push(sample);
    }
    if stream.has_power() {
        report.power_dropout_seconds = stream.power_dropout().iter().filter(|d| **d).count();
    }
    (stream, report)
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    elapsed: u32,
    record: Option<usize>,
    segment: u32,
}

#[derive(Debug, Clone, Copy)]
struct Pause {
    stop: u32,
    /// First timestamp after the pause; `u32::MAX` if the timer never
    /// restarted.
    start: u32,
}

/// Timer pauses as `[stop, start)` intervals.
fn timer_pauses(timer: &[TimerEvent]) -> Vec<Pause> {
    let mut pauses = Vec::new();
    let mut stopped_at: Option<u32> = None;
    for event in timer {
        match (event.kind, stopped_at) {
            (TimerEventKind::Stop, None) => stopped_at = Some(event.timestamp),
            (TimerEventKind::Start, Some(stop)) => {
                pauses.push(Pause {
                    stop,
                    start: event.timestamp,
                });
                stopped_at = None;
            }
            _ => {}
        }
    }
    if let Some(stop) = stopped_at {
        pauses.push(Pause {
            stop,
            start: u32::MAX,
        });
    }
    pauses
}

/// Applies the spike, timer, out-of-order and duplicate rules. The result
/// has strictly increasing timestamps.
fn merge_records(
    records: &[RawRecord],
    pauses: &[Pause],
    config: &ResampleConfig,
    report: &mut ResampleReport,
) -> Vec<RawRecord> {
    let mut merged: Vec<RawRecord> = Vec::with_capacity(records.len());
    for record in records {
        let mut record = *record;
        if let (Some(p), Some(max)) = (record.power, config.max_power) {
            if p > max {
                record.power = None;
                report.power_spikes = report.power_spikes.saturating_add(1);
            }
        }
        if pauses
            .iter()
            .any(|p| record.timestamp >= p.stop && record.timestamp < p.start)
        {
            report.timer_paused_records = report.timer_paused_records.saturating_add(1);
            continue;
        }
        match merged.last_mut() {
            Some(last) if record.timestamp == last.timestamp => {
                last.merge(&record);
                report.duplicates = report.duplicates.saturating_add(1);
            }
            Some(last) if record.timestamp < last.timestamp => {
                report.out_of_order = report.out_of_order.saturating_add(1);
            }
            _ => merged.push(record),
        }
    }
    merged
}

/// `start..end` index ranges of consecutive slots in the same segment.
fn segment_ranges(slots: &[Slot]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (i, pair) in slots.windows(2).enumerate() {
        if let [a, b] = pair {
            if a.segment != b.segment {
                let end = i.saturating_add(1);
                ranges.push((start, end));
                start = end;
            }
        }
    }
    if start < slots.len() {
        ranges.push((start, slots.len()));
    }
    ranges
}

/// Fills runs of `None` in each segment of `values` (see the module docs)
/// and returns how many *recorded* seconds were repaired.
fn fill_channel(
    values: &mut [Option<f64>],
    recorded: &[bool],
    segments: &[(usize, usize)],
    max_dropout: usize,
) -> usize {
    let mut repaired = 0usize;
    for &(start, end) in segments {
        let (Some(values), Some(recorded)) = (values.get_mut(start..end), recorded.get(start..end))
        else {
            continue;
        };
        repaired = repaired.saturating_add(fill_segment(values, recorded, max_dropout));
    }
    repaired
}

fn fill_segment(values: &mut [Option<f64>], recorded: &[bool], max_dropout: usize) -> usize {
    let mut repaired = 0usize;
    let mut i = 0usize;
    while i < values.len() {
        if values.get(i).is_some_and(Option::is_some) {
            i = i.saturating_add(1);
            continue;
        }
        let run_start = i;
        let mut run_end = i;
        while values.get(run_end).is_some_and(Option::is_none) {
            run_end = run_end.saturating_add(1);
        }
        i = run_end;

        let dropout = recorded
            .get(run_start..run_end)
            .map_or(0, |r| r.iter().filter(|x| **x).count());
        if dropout > max_dropout {
            continue;
        }
        let left = run_start
            .checked_sub(1)
            .and_then(|k| values.get(k))
            .copied()
            .flatten();
        let right = values.get(run_end).copied().flatten();
        let Some(run) = values.get_mut(run_start..run_end) else {
            continue;
        };
        match (left, right) {
            (Some(a), Some(b)) => {
                // Points 1..=len of a line from `a` (index 0) to `b`
                // (index len + 1).
                let span = f64_from_usize(run.len().saturating_add(1));
                for (k, slot) in (1usize..).zip(run.iter_mut()) {
                    *slot = Some(a + (b - a) * f64_from_usize(k) / span);
                }
            }
            (Some(v), None) | (None, Some(v)) => run.fill(Some(v)),
            (None, None) => continue,
        }
        repaired = repaired.saturating_add(dropout);
    }
    repaired
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn p(t: u32, w: u16) -> RawRecord {
        RawRecord {
            power: Some(w),
            ..RawRecord::at(t)
        }
    }

    fn run(records: &[RawRecord]) -> (ActivityStream, ResampleReport) {
        resample(records, &[], &ResampleConfig::default())
    }

    #[test]
    fn empty_input() {
        let (s, r) = run(&[]);
        assert!(s.is_empty());
        assert_eq!(r, ResampleReport::default());
    }

    #[test]
    fn consecutive_records_pass_through() {
        let (s, r) = run(&[p(10, 1), p(11, 2), p(12, 3)]);
        assert_eq!(s.power(), &[1, 2, 3]);
        assert_eq!(s.elapsed(), &[0, 1, 2]);
        assert_eq!(s.start_time(), 10);
        assert!(s.state().iter().all(|st| *st == SampleState::Recorded));
        assert_eq!(r.gaps_filled + r.pauses + r.duplicates, 0);
    }

    #[test]
    fn duplicates_merge_field_by_field() {
        let a = RawRecord {
            power: Some(100),
            heart_rate: Some(120),
            ..RawRecord::at(5)
        };
        let b = RawRecord {
            power: Some(200),
            cadence: Some(90),
            ..RawRecord::at(5)
        };
        let (s, r) = run(&[a, b]);
        assert_eq!(s.power(), &[200]);
        assert_eq!(s.heart_rate(), &[Some(120)]);
        assert_eq!(s.cadence(), &[Some(90)]);
        assert_eq!(r.duplicates, 1);
    }

    #[test]
    fn out_of_order_dropped() {
        let (s, r) = run(&[p(10, 1), p(9, 99), p(11, 2)]);
        assert_eq!(s.power(), &[1, 2]);
        assert_eq!(r.out_of_order, 1);
    }

    #[test]
    fn short_gap_interpolates_every_channel() {
        let a = RawRecord {
            power: Some(100),
            heart_rate: Some(100),
            speed: Some(5.0),
            distance: Some(0.0),
            ..RawRecord::at(0)
        };
        let b = RawRecord {
            power: Some(400),
            heart_rate: Some(130),
            speed: Some(8.0),
            distance: Some(30.0),
            ..RawRecord::at(3)
        };
        let (s, r) = run(&[a, b]);
        assert_eq!(s.power(), &[100, 200, 300, 400]);
        assert_eq!(
            s.heart_rate(),
            &[Some(100), Some(110), Some(120), Some(130)]
        );
        assert_eq!(s.speed(), &[Some(5.0), Some(6.0), Some(7.0), Some(8.0)]);
        assert_eq!(s.distance()[2], Some(20.0));
        assert_eq!(s.state()[1], SampleState::Filled);
        assert_eq!((r.gaps_filled, r.gap_seconds_filled), (1, 2));
        // A gap is not a dropout: nothing was recorded there.
        assert_eq!(r.power_dropouts_repaired, 0);
    }

    #[test]
    fn gap_of_exactly_pause_gap_is_filled_longer_is_pause() {
        let (s, r) = run(&[p(0, 100), p(31, 100)]); // 30 missing seconds
        assert_eq!(s.len(), 32);
        assert_eq!(r.pauses, 0);
        let (s, r) = run(&[p(0, 100), p(32, 100)]); // 31 missing seconds
        assert_eq!(s.len(), 2);
        assert_eq!(s.elapsed(), &[0, 32]);
        assert_eq!((r.pauses, r.paused_seconds), (1, 31));
        assert_eq!(s.segment_starts(), &[1]);
    }

    #[test]
    fn interpolation_does_not_cross_a_pause() {
        let mut recs = vec![p(0, 100), RawRecord::at(1)];
        recs.push(p(100, 300));
        let (s, _) = run(&recs);
        // The missing power at t=1 is held from the left, not interpolated
        // towards the record after the pause.
        assert_eq!(s.power(), &[100, 100, 300]);
    }

    #[test]
    fn dropout_up_to_limit_is_repaired() {
        let mut recs = vec![p(0, 100)];
        recs.extend((1..=8).map(RawRecord::at));
        recs.push(p(9, 280));
        let (s, r) = run(&recs);
        assert_eq!(
            s.power(),
            &[100, 120, 140, 160, 180, 200, 220, 240, 260, 280]
        );
        assert!(s.power_dropout().iter().all(|d| !d));
        assert_eq!((r.power_dropouts_repaired, r.power_dropout_seconds), (8, 0));
    }

    #[test]
    fn dropout_over_limit_becomes_zero() {
        let mut recs = vec![p(0, 100)];
        recs.extend((1..=9).map(RawRecord::at));
        recs.push(p(10, 100));
        let (s, r) = run(&recs);
        assert_eq!(&s.power()[1..10], &[0; 9]);
        assert_eq!(r.power_dropout_seconds, 9);
        assert_eq!(r.power_dropouts_repaired, 0);
    }

    #[test]
    fn long_hr_dropout_stays_missing() {
        let hr = |t, v| RawRecord {
            heart_rate: v,
            ..RawRecord::at(t)
        };
        let mut recs = vec![hr(0, Some(120))];
        recs.extend((1..=20).map(|t| hr(t, None)));
        recs.push(hr(21, Some(130)));
        let (s, _) = run(&recs);
        assert_eq!(s.heart_rate()[10], None);
        assert!(!s.has_power());
    }

    #[test]
    fn dropout_at_segment_edge_is_held() {
        let (s, _) = run(&[RawRecord::at(0), p(1, 150), p(2, 160), RawRecord::at(3)]);
        assert_eq!(s.power(), &[150, 150, 160, 160]);
    }

    #[test]
    fn spikes_are_removed_and_repaired() {
        let (s, r) = run(&[p(0, 200), p(1, 4000), p(2, 300)]);
        assert_eq!(s.power(), &[200, 250, 300]);
        assert_eq!(r.power_spikes, 1);
        let no_cap = ResampleConfig {
            max_power: None,
            ..ResampleConfig::default()
        };
        let (s, _) = resample(&[p(0, 200), p(1, 4000)], &[], &no_cap);
        assert_eq!(s.power(), &[200, 4000]);
    }

    #[test]
    fn timer_events_ignored_by_default() {
        let timer = [
            TimerEvent {
                timestamp: 2,
                kind: TimerEventKind::Stop,
            },
            TimerEvent {
                timestamp: 4,
                kind: TimerEventKind::Start,
            },
        ];
        let recs: Vec<_> = (0..6).map(|t| p(t, 100)).collect();
        let (s, _) = resample(&recs, &timer, &ResampleConfig::default());
        assert_eq!(s.len(), 6);
        let cfg = ResampleConfig {
            use_timer_events: true,
            ..ResampleConfig::default()
        };
        let (s, r) = resample(&recs, &timer, &cfg);
        assert_eq!(s.elapsed(), &[0, 1, 4, 5]);
        assert_eq!(r.timer_paused_records, 2);
        assert_eq!(r.pauses, 1);
    }

    #[test]
    fn short_timer_pause_still_breaks_segment() {
        let timer = [
            TimerEvent {
                timestamp: 3,
                kind: TimerEventKind::Stop,
            },
            TimerEvent {
                timestamp: 4,
                kind: TimerEventKind::Start,
            },
        ];
        let cfg = ResampleConfig {
            use_timer_events: true,
            ..ResampleConfig::default()
        };
        // Records at 0..=2, then 5 (gap of 2 s, normally filled).
        let recs = [p(0, 1), p(1, 1), p(2, 1), p(5, 9)];
        let (s, r) = resample(&recs, &timer, &cfg);
        assert_eq!(s.elapsed(), &[0, 1, 2, 5]);
        assert_eq!(r.gaps_filled, 0);
    }

    #[test]
    fn timer_never_restarted_drops_the_rest() {
        let timer = [TimerEvent {
            timestamp: 2,
            kind: TimerEventKind::Stop,
        }];
        let cfg = ResampleConfig {
            use_timer_events: true,
            ..ResampleConfig::default()
        };
        let recs: Vec<_> = (0..5).map(|t| p(t, 100)).collect();
        let (s, _) = resample(&recs, &timer, &cfg);
        assert_eq!(s.len(), 2);
    }
}
