//! The 1 Hz activity stream every metric is computed from.

use alloc::vec::Vec;

/// Default speed threshold for [`ActivityStream::moving_time`], m/s.
pub const DEFAULT_MOVING_SPEED: f64 = 0.5;

/// Where a sample's values came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum SampleState {
    /// The device wrote a record for this second.
    Recorded,
    /// No record for this second (a short gap, e.g. smart recording). The
    /// values were interpolated from the records around it.
    Filled,
}

/// One second of an [`ActivityStream`], as passed to
/// [`ActivityStream::push`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Seconds since the first sample of the activity, counting paused time.
    pub elapsed: u32,
    /// Power, W. `None` is a dropout and is stored as 0 W.
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
    /// Where the values came from.
    pub state: SampleState,
}

impl Sample {
    /// A recorded sample with only `elapsed` and `power` set.
    ///
    /// ```
    /// use zerofit_analytics::stream::Sample;
    /// let s = Sample::power(3, 250);
    /// assert_eq!((s.elapsed, s.power, s.heart_rate), (3, Some(250), None));
    /// ```
    #[must_use]
    pub const fn power(elapsed: u32, watts: u16) -> Self {
        Self {
            elapsed,
            power: Some(watts),
            heart_rate: None,
            cadence: None,
            speed: None,
            altitude: None,
            distance: None,
            state: SampleState::Recorded,
        }
    }
}

/// An activity resampled to one sample per second of *recording time*.
///
/// Paused time is not in the stream: sample `i` is the `i`-th second the
/// athlete was recording, and [`elapsed`](Self::elapsed) maps it back to
/// wall-clock time. Channels are stored as separate vectors
/// (struct-of-arrays), so the power metrics can take `&[u16]` directly and
/// iterate over contiguous memory.
///
/// Power is stored as `u16` with dropouts set to 0 W (and flagged in
/// [`power_dropout`](Self::power_dropout)), because every power metric needs
/// a value for every second. The other channels keep `None` for missing
/// values, and their metrics skip those seconds.
///
/// Build one with [`resample`](crate::resample::resample) from raw records,
/// or directly:
///
/// ```
/// use zerofit_analytics::ActivityStream;
/// let stream = ActivityStream::from_power(0, &[100, 200, 300]);
/// assert_eq!(stream.len(), 3);
/// assert_eq!(stream.power(), &[100, 200, 300]);
/// assert_eq!(stream.recording_time(), 3);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActivityStream {
    start_time: u32,
    elapsed: Vec<u32>,
    power: Vec<u16>,
    power_dropout: Vec<bool>,
    has_power: bool,
    heart_rate: Vec<Option<u8>>,
    cadence: Vec<Option<u8>>,
    speed: Vec<Option<f64>>,
    altitude: Vec<Option<f64>>,
    distance: Vec<Option<f64>>,
    state: Vec<SampleState>,
    segment_starts: Vec<usize>,
}

/// The error returned by [`ActivityStream::push`] when a sample's
/// `elapsed` is not after the previous sample's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotIncreasing {
    /// `elapsed` of the previous sample.
    pub previous: u32,
    /// `elapsed` of the rejected sample.
    pub elapsed: u32,
}

impl core::fmt::Display for NotIncreasing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "sample elapsed time {} is not after the previous sample ({})",
            self.elapsed, self.previous
        )
    }
}

impl core::error::Error for NotIncreasing {}

impl ActivityStream {
    /// An empty stream whose first sample is at FIT time `start_time`
    /// (seconds since 1989-12-31T00:00:00Z; 0 if unknown).
    #[must_use]
    pub fn new(start_time: u32) -> Self {
        Self {
            start_time,
            ..Self::default()
        }
    }

    /// A stream of consecutive recorded seconds with power only.
    ///
    /// ```
    /// use zerofit_analytics::ActivityStream;
    /// let s = ActivityStream::from_power(0, &[0, 0, 300]);
    /// assert!(s.has_power());
    /// assert_eq!(s.elapsed(), &[0, 1, 2]);
    /// ```
    #[must_use]
    pub fn from_power(start_time: u32, power: &[u16]) -> Self {
        let mut stream = Self::new(start_time);
        stream.reserve(power.len());
        for (elapsed, watts) in (0u32..).zip(power) {
            stream.push_unchecked(Sample::power(elapsed, *watts));
        }
        stream
    }

    /// A stream of consecutive recorded seconds with power and heart rate.
    /// The longer slice is truncated to the shorter one's length.
    ///
    /// ```
    /// use zerofit_analytics::ActivityStream;
    /// let s = ActivityStream::from_power_hr(0, &[200, 210], &[140, 141]);
    /// assert_eq!(s.heart_rate(), &[Some(140), Some(141)]);
    /// ```
    #[must_use]
    pub fn from_power_hr(start_time: u32, power: &[u16], heart_rate: &[u8]) -> Self {
        let mut stream = Self::new(start_time);
        stream.reserve(power.len().min(heart_rate.len()));
        for ((elapsed, watts), hr) in (0u32..).zip(power).zip(heart_rate) {
            let mut sample = Sample::power(elapsed, *watts);
            sample.heart_rate = Some(*hr);
            stream.push_unchecked(sample);
        }
        stream
    }

    /// Reserves capacity for `additional` more samples in every channel.
    pub fn reserve(&mut self, additional: usize) {
        self.elapsed.reserve(additional);
        self.power.reserve(additional);
        self.power_dropout.reserve(additional);
        self.heart_rate.reserve(additional);
        self.cadence.reserve(additional);
        self.speed.reserve(additional);
        self.altitude.reserve(additional);
        self.distance.reserve(additional);
        self.state.reserve(additional);
    }

    /// Appends one second.
    ///
    /// If `sample.elapsed` is more than one second after the previous
    /// sample, the time between them was a pause: it is recorded as a
    /// segment boundary (see [`segment_starts`](Self::segment_starts)).
    ///
    /// # Errors
    ///
    /// [`NotIncreasing`] if `sample.elapsed` is not after the previous
    /// sample's; the stream is unchanged.
    ///
    /// ```
    /// use zerofit_analytics::{ActivityStream, stream::Sample};
    /// let mut s = ActivityStream::new(0);
    /// s.push(Sample::power(0, 100))?;
    /// s.push(Sample::power(60, 100))?; // after a 59 s pause
    /// assert_eq!(s.segment_starts(), &[1]);
    /// assert!(s.push(Sample::power(60, 100)).is_err());
    /// # Ok::<(), zerofit_analytics::stream::NotIncreasing>(())
    /// ```
    pub fn push(&mut self, sample: Sample) -> Result<(), NotIncreasing> {
        if let Some(&previous) = self.elapsed.last() {
            if sample.elapsed <= previous {
                return Err(NotIncreasing {
                    previous,
                    elapsed: sample.elapsed,
                });
            }
            if sample.elapsed > previous.saturating_add(1) {
                self.segment_starts.push(self.elapsed.len());
            }
        }
        self.push_unchecked(sample);
        Ok(())
    }

    fn push_unchecked(&mut self, sample: Sample) {
        self.elapsed.push(sample.elapsed);
        self.power.push(sample.power.unwrap_or(0));
        self.power_dropout.push(sample.power.is_none());
        self.has_power |= sample.power.is_some();
        self.heart_rate.push(sample.heart_rate);
        self.cadence.push(sample.cadence);
        self.speed.push(sample.speed);
        self.altitude.push(sample.altitude);
        self.distance.push(sample.distance);
        self.state.push(sample.state);
    }

    /// Number of samples (seconds of recording time).
    #[must_use]
    pub fn len(&self) -> usize {
        self.elapsed.len()
    }

    /// Whether the stream has no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elapsed.is_empty()
    }

    /// FIT time of the first sample (0 if unknown).
    #[must_use]
    pub const fn start_time(&self) -> u32 {
        self.start_time
    }

    /// Recording time in seconds: the number of samples. This is the
    /// duration TSS, average power and the zone times use.
    #[must_use]
    pub fn recording_time(&self) -> usize {
        self.len()
    }

    /// Elapsed (wall-clock) time in seconds, from the first sample to the
    /// end of the last one, including pauses.
    #[must_use]
    pub fn elapsed_time(&self) -> u32 {
        self.elapsed.last().map_or(0, |e| e.saturating_add(1))
    }

    /// Moving time in seconds: samples with speed above `min_speed` m/s.
    ///
    /// Samples without a speed value count as moving, so an indoor ride
    /// with no speed sensor is all moving time, which is also how
    /// intervals.icu treats activities without a usable velocity stream.
    /// Judgment call: the threshold. intervals.icu derives moving time from
    /// the velocity stream but doesn't publish its threshold; the crate
    /// default [`DEFAULT_MOVING_SPEED`] (0.5 m/s, 1.8 km/h, slower than
    /// walking) treats only real standstills (traffic lights, café stops
    /// with the timer running) as stopped.
    ///
    /// ```
    /// use zerofit_analytics::{ActivityStream, stream::Sample};
    /// let mut s = ActivityStream::new(0);
    /// for (t, v) in [(0, Some(8.0)), (1, Some(0.0)), (2, None)] {
    ///     s.push(Sample { speed: v, ..Sample::power(t, 100) })?;
    /// }
    /// assert_eq!(s.moving_time(0.5), 2);
    /// # Ok::<(), zerofit_analytics::stream::NotIncreasing>(())
    /// ```
    #[must_use]
    pub fn moving_time(&self, min_speed: f64) -> usize {
        self.speed
            .iter()
            .filter(|v| v.is_none_or(|v| v > min_speed))
            .count()
    }

    /// Seconds since the first sample, per sample. Strictly increasing.
    #[must_use]
    pub fn elapsed(&self) -> &[u32] {
        &self.elapsed
    }

    /// Power, W, per sample; dropouts are 0.
    #[must_use]
    pub fn power(&self) -> &[u16] {
        &self.power
    }

    /// Whether each sample's power is a dropout (stored as 0 W).
    #[must_use]
    pub fn power_dropout(&self) -> &[bool] {
        &self.power_dropout
    }

    /// Whether any sample has a power value (a power meter was present).
    #[must_use]
    pub const fn has_power(&self) -> bool {
        self.has_power
    }

    /// Heart rate, bpm, per sample.
    #[must_use]
    pub fn heart_rate(&self) -> &[Option<u8>] {
        &self.heart_rate
    }

    /// Cadence, rpm, per sample.
    #[must_use]
    pub fn cadence(&self) -> &[Option<u8>] {
        &self.cadence
    }

    /// Speed, m/s, per sample.
    #[must_use]
    pub fn speed(&self) -> &[Option<f64>] {
        &self.speed
    }

    /// Altitude, m, per sample.
    #[must_use]
    pub fn altitude(&self) -> &[Option<f64>] {
        &self.altitude
    }

    /// Distance, m, per sample.
    #[must_use]
    pub fn distance(&self) -> &[Option<f64>] {
        &self.distance
    }

    /// Where each sample came from.
    #[must_use]
    pub fn state(&self) -> &[SampleState] {
        &self.state
    }

    /// Indices of the first sample after each pause. Rolling windows (NP,
    /// MMP) run across these boundaries, because the stream is recording
    /// time; this list lets callers split by segment instead.
    #[must_use]
    pub fn segment_starts(&self) -> &[usize] {
        &self.segment_starts
    }

    /// Power over *elapsed* time: the recorded power with every paused
    /// second filled with 0 W. intervals.icu builds its power curve this way
    /// (a 4 h curve point is total work / elapsed time, stops included), so
    /// [`analyze_stream`](crate::analyze_stream) uses it for the power curve
    /// by default.
    ///
    /// ```
    /// use zerofit_analytics::{ActivityStream, stream::Sample};
    /// let mut s = ActivityStream::new(0);
    /// s.push(Sample::power(0, 100))?;
    /// s.push(Sample::power(3, 200))?;
    /// assert_eq!(s.power_elapsed(), vec![100, 0, 0, 200]);
    /// # Ok::<(), zerofit_analytics::stream::NotIncreasing>(())
    /// ```
    #[must_use]
    pub fn power_elapsed(&self) -> Vec<u16> {
        let total = usize::try_from(self.elapsed_time()).unwrap_or(usize::MAX);
        let mut out = Vec::with_capacity(total.min(self.len().saturating_mul(4)));
        for (&e, &p) in self.elapsed.iter().zip(&self.power) {
            let e = usize::try_from(e).unwrap_or(usize::MAX);
            out.resize(e.max(out.len()), 0);
            out.push(p);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropouts_are_zero_and_flagged() {
        let mut s = ActivityStream::new(5);
        let mut sample = Sample::power(0, 100);
        sample.power = None;
        s.push(sample).unwrap();
        assert_eq!(s.power(), &[0]);
        assert_eq!(s.power_dropout(), &[true]);
        assert!(!s.has_power());
        assert_eq!(s.start_time(), 5);
    }

    #[test]
    fn elapsed_and_recording_time() {
        let mut s = ActivityStream::new(0);
        s.push(Sample::power(0, 1)).unwrap();
        s.push(Sample::power(1, 1)).unwrap();
        s.push(Sample::power(10, 1)).unwrap();
        assert_eq!(s.recording_time(), 3);
        assert_eq!(s.elapsed_time(), 11);
        assert_eq!(s.segment_starts(), &[2]);
        assert_eq!(s.power_elapsed().len(), 11);
    }

    #[test]
    fn empty() {
        let s = ActivityStream::new(0);
        assert!(s.is_empty());
        assert_eq!(s.elapsed_time(), 0);
        assert!(s.power_elapsed().is_empty());
    }
}
