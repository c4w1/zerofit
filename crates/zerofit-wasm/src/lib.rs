//! WebAssembly build of the zerofit crates for the web app: decoding and
//! analysis (`zerofit`, `zerofit-profile`, `zerofit-analytics`), fitness
//! across activities, structured workouts, and fueling plans
//! (`zerofit-fueling`), in one module.
//!
//! ```sh
//! cargo build -p zerofit-wasm --profile wasm-release --target wasm32-unknown-unknown
//! wasm-bindgen --target web --out-dir web/src/lib/wasm/pkg \
//!     target/wasm32-unknown-unknown/wasm-release/zerofit_wasm.wasm
//! ```
//!
//! The web app loads it inside a Web Worker (see `web/src/lib/worker`).
//! Every export is a thin wrapper around a plain Rust function in this
//! crate (`*_json`, [`analyze_activity`]), which the native tests cover;
//! the wrappers only convert errors to `JsError`. Inputs and summaries
//! cross the boundary as JSON; per-second streams cross as typed arrays
//! (`Vec<u16>` becomes a `Uint16Array`), which the worker transfers to
//! the UI thread without copying.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::wasm_bindgen;
use zerofit_analytics::cp::{CpFit, fit_2p, fit_3p};
use zerofit_analytics::fit::{Sport, read_fit};
use zerofit_analytics::load::{
    DailyLoad, EFTP_RANGE, EftpEstimate, LoadConfig, daily_loads, estimate_ftp_from_points,
    training_load,
};
use zerofit_analytics::mmp::PowerCurve;
use zerofit_analytics::summary::ActivitySummary;
use zerofit_analytics::workout::{PlannedLoad, StepKind, StepTarget, Workout, WorkoutStep};
use zerofit_analytics::{
    AnalysisConfig, AthleteSettings, DEFAULT_W_PRIME, TrimpCoefficients, analyze_records,
};
use zerofit_fueling::{
    Athlete, DayAhead, DayInput, DayPlan, MealSchedule, PlannedSession, Priority,
};

// ---------------------------------------------------------------- settings

/// Athlete settings as accepted from JavaScript. Every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// FTP, W.
    pub ftp: Option<f64>,
    /// Body weight, kg.
    pub weight_kg: Option<f64>,
    /// Lactate threshold heart rate, bpm.
    pub lthr: Option<u8>,
    /// Maximum heart rate, bpm.
    pub max_hr: Option<u8>,
    /// Resting heart rate, bpm.
    pub resting_hr: Option<u8>,
    /// Critical power, W.
    pub cp: Option<f64>,
    /// W', J.
    pub w_prime: Option<f64>,
    /// `"male"` (default) or `"female"` TRIMP coefficients.
    pub trimp: Option<String>,
    /// Power zone upper bounds as fractions of FTP (default Coggan).
    pub power_zones: Option<Vec<f64>>,
    /// HR zone upper bounds as fractions of LTHR (default Friel).
    pub hr_zones: Option<Vec<f64>>,
}

impl Settings {
    fn parse(json: &str) -> Result<Self, String> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json).map_err(|e| format!("invalid settings: {e}"))
    }

    fn to_athlete(&self) -> Result<AthleteSettings, String> {
        let trimp = match self.trimp.as_deref() {
            None | Some("male") => TrimpCoefficients::Male,
            Some("female") => TrimpCoefficients::Female,
            Some(other) => return Err(format!("unknown trimp coefficients {other:?}")),
        };
        let mut athlete = AthleteSettings {
            ftp: self.ftp,
            weight_kg: self.weight_kg,
            lthr: self.lthr,
            max_hr: self.max_hr,
            resting_hr: self.resting_hr,
            trimp,
            cp: self.cp,
            w_prime: self.w_prime,
            ..AthleteSettings::default()
        };
        if let Some(z) = &self.power_zones {
            athlete.power_zones = zerofit_analytics::Zones::new(z.clone())
                .ok_or("power zones must be positive and increasing")?;
        }
        if let Some(z) = &self.hr_zones {
            athlete.hr_zones = zerofit_analytics::Zones::new(z.clone())
                .ok_or("HR zones must be positive and increasing")?;
        }
        Ok(athlete)
    }
}

// ---------------------------------------------------------------- analysis

/// Durations at which an activity's power curve is stored and sent to the
/// UI: every second to 20 s, then steps of ~10 % up to the ride length
/// (about 90 points for a 6 h ride). A season curve is the pointwise max
/// over activities on this shared grid.
///
/// ```
/// let grid = zerofit_wasm::curve_grid(3600);
/// assert_eq!(&grid[..3], &[1, 2, 3]);
/// assert_eq!(grid.last(), Some(&3600));
/// assert!(grid.windows(2).all(|w| w[1] > w[0]));
/// assert_eq!(grid.len(), 73); // 20 one-second steps, then ~10 % steps
/// ```
#[must_use]
pub fn curve_grid(max_s: usize) -> Vec<usize> {
    let mut out: Vec<usize> = (1..=max_s.min(20)).collect();
    let mut d = 20usize;
    while d < max_s {
        // ceil(d × 1.1) without floats: d + ceil(d / 10).
        d = d.saturating_add(d.div_ceil(10)).min(max_s);
        out.push(d);
    }
    out
}

/// Everything the UI needs about one activity.
#[derive(Debug, Default)]
#[wasm_bindgen]
pub struct ActivityResult {
    summary_json: String,
    start_time: u32,
    elapsed: Vec<u32>,
    power: Vec<u16>,
    heart_rate: Vec<u8>,
    cadence: Vec<u8>,
    altitude: Vec<f32>,
    speed: Vec<f32>,
    w_prime_balance: Vec<f32>,
    curve_durations: Vec<u32>,
    curve_watts: Vec<f32>,
}

#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
impl ActivityResult {
    /// `{"sport": …, "summary": ActivitySummary}` as JSON.
    #[must_use]
    pub fn summary_json(&self) -> String {
        self.summary_json.clone()
    }
    /// FIT time of the first sample (s since 1989-12-31; 0 if unknown).
    #[must_use]
    pub fn start_time(&self) -> u32 {
        self.start_time
    }
    /// Seconds since the first sample, per sample (moves the data out).
    pub fn take_elapsed(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.elapsed)
    }
    /// Power, W, per sample (dropouts 0).
    pub fn take_power(&mut self) -> Vec<u16> {
        core::mem::take(&mut self.power)
    }
    /// Heart rate, bpm, per sample (0 = missing).
    pub fn take_heart_rate(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.heart_rate)
    }
    /// Cadence, rpm, per sample (0 = missing or coasting).
    pub fn take_cadence(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.cadence)
    }
    /// Altitude, m, per sample (NaN = missing).
    pub fn take_altitude(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.altitude)
    }
    /// Speed, m/s, per sample (NaN = missing).
    pub fn take_speed(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.speed)
    }
    /// W' balance, J, per sample (empty without power or CP/FTP).
    pub fn take_w_prime_balance(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.w_prime_balance)
    }
    /// Power-curve durations, s ([`curve_grid`]).
    pub fn take_curve_durations(&mut self) -> Vec<u32> {
        core::mem::take(&mut self.curve_durations)
    }
    /// Power-curve watts at those durations.
    pub fn take_curve_watts(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.curve_watts)
    }
}

#[derive(Serialize)]
struct SummaryOut<'a> {
    sport: Option<&'static str>,
    summary: &'a ActivitySummary,
}

#[allow(clippy::cast_possible_truncation)] // f64 → f32 for display streams
fn to_f32(v: Option<f64>) -> f32 {
    v.map_or(f32::NAN, |x| x as f32)
}

/// Decodes and analyzes one FIT file.
///
/// # Errors
///
/// Invalid settings JSON or a FIT decoding error, as a message.
pub fn analyze_activity(fit_bytes: &[u8], settings_json: &str) -> Result<ActivityResult, String> {
    let athlete = Settings::parse(settings_json)?.to_athlete()?;
    let activity = read_fit(fit_bytes).map_err(|e| format!("FIT decoding failed: {e}"))?;
    let config = AnalysisConfig::default();
    let (stream, report) = zerofit_analytics::resample::resample(
        &activity.records,
        &activity.timer_events,
        &config.resample,
    );
    let mut analysis = zerofit_analytics::analyze_stream(&stream, &athlete, &config);
    analysis.summary.resample_report = Some(report);

    let grid = curve_grid(analysis.power_curve.max_duration());
    let (curve_durations, curve_watts) = curve_points(&analysis.power_curve, &grid);
    let out = SummaryOut {
        sport: activity.sport.map(sport_name),
        summary: &analysis.summary,
    };
    Ok(ActivityResult {
        summary_json: serde_json::to_string(&out).map_err(|e| e.to_string())?,
        start_time: stream.start_time(),
        elapsed: stream.elapsed().to_vec(),
        power: stream.power().to_vec(),
        heart_rate: stream.heart_rate().iter().map(|h| h.unwrap_or(0)).collect(),
        cadence: stream.cadence().iter().map(|c| c.unwrap_or(0)).collect(),
        altitude: stream.altitude().iter().map(|a| to_f32(*a)).collect(),
        speed: stream.speed().iter().map(|v| to_f32(*v)).collect(),
        w_prime_balance: analysis
            .w_prime_balance
            .iter()
            .map(|b| to_f32(Some(*b)))
            .collect(),
        curve_durations,
        curve_watts,
    })
}

fn curve_points(curve: &PowerCurve, grid: &[usize]) -> (Vec<u32>, Vec<f32>) {
    grid.iter()
        .filter_map(|&d| Some((u32::try_from(d).ok()?, to_f32(curve.watts(d)))))
        .unzip()
}

const fn sport_name(s: Sport) -> &'static str {
    match s {
        Sport::Cycling => "cycling",
        Sport::Running => "running",
        Sport::Other => "other",
    }
}

/// The summary only, as JSON: what the node smoke test checks.
///
/// # Errors
///
/// As [`analyze_activity`].
///
/// ```
/// let err = zerofit_wasm::analyze_json(b"not a fit file", "").unwrap_err();
/// assert!(err.starts_with("FIT decoding failed"));
/// ```
pub fn analyze_json(fit_bytes: &[u8], settings_json: &str) -> Result<String, String> {
    let athlete = Settings::parse(settings_json)?.to_athlete()?;
    let activity = read_fit(fit_bytes).map_err(|e| format!("FIT decoding failed: {e}"))?;
    let analysis = analyze_records(
        &activity.records,
        &activity.timer_events,
        &athlete,
        &AnalysisConfig::default(),
    );
    serde_json::to_string(&SummaryOut {
        sport: activity.sport.map(sport_name),
        summary: &analysis.summary,
    })
    .map_err(|e| e.to_string())
}

// ----------------------------------------------------------------- fitness

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FitnessIn {
    /// Each activity's day (any integer day number, e.g. days since
    /// 1970-01-01 in local time), TSS and stored curve.
    activities: Vec<FitnessActivity>,
    /// Last day of the series (e.g. today).
    end_day: i64,
    /// W' for eFTP when the season curve can't fit one.
    #[serde(default)]
    w_prime_fallback: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FitnessActivity {
    day: i64,
    tss: Option<f64>,
    #[serde(default)]
    curve: Vec<(usize, f64)>,
}

#[derive(Serialize)]
struct FitnessOut {
    first_day: i64,
    days: Vec<DailyLoad>,
    season_curve: Vec<(usize, f64)>,
    cp_2p: Option<CpFit>,
    cp_3p: Option<CpFit>,
    eftp: Option<EftpEstimate>,
}

/// CTL/ATL/TSB from the first activity's day to `end_day`, the season
/// curve (pointwise max on the shared grid), CP fits and eFTP.
///
/// # Errors
///
/// Invalid JSON.
pub fn fitness_json(input_json: &str) -> Result<String, String> {
    let input: FitnessIn =
        serde_json::from_str(input_json).map_err(|e| format!("invalid input: {e}"))?;
    let first_day = input
        .activities
        .iter()
        .map(|a| a.day)
        .min()
        .unwrap_or(input.end_day)
        .min(input.end_day);
    let span = usize::try_from(input.end_day.saturating_sub(first_day)).unwrap_or(0);
    let mut per_day: Vec<(usize, f64)> = input
        .activities
        .iter()
        .filter_map(|a| {
            let d = usize::try_from(a.day.saturating_sub(first_day)).ok()?;
            (d <= span).then_some((d, a.tss.unwrap_or(0.0)))
        })
        .collect();
    per_day.push((span, 0.0)); // extend the series to end_day
    let days = training_load(&daily_loads(&per_day), &LoadConfig::default());

    let mut season: std::collections::BTreeMap<usize, f64> = std::collections::BTreeMap::new();
    for a in &input.activities {
        for &(d, w) in &a.curve {
            if w.is_finite() {
                let best = season.entry(d).or_insert(w);
                *best = best.max(w);
            }
        }
    }
    let season_curve: Vec<(usize, f64)> = season.into_iter().collect();
    let points_in = |lo: usize, hi: usize| -> Vec<(f64, f64)> {
        season_curve
            .iter()
            .filter(|(d, _)| (lo..=hi).contains(d))
            .map(|&(d, w)| (f64::from(u32::try_from(d).unwrap_or(u32::MAX)), w))
            .collect()
    };
    let out = FitnessOut {
        first_day,
        days,
        cp_2p: fit_2p(&points_in(120, 1200)),
        cp_3p: fit_3p(&points_in(10, 1200)),
        eftp: estimate_ftp_from_points(
            &season_curve,
            EFTP_RANGE,
            input.w_prime_fallback.unwrap_or(DEFAULT_W_PRIME),
        ),
        season_curve,
    };
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- workouts

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkoutIn {
    name: String,
    steps: Vec<StepIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepIn {
    duration_s: u32,
    #[serde(default)]
    kind: KindIn,
    target: TargetIn,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum KindIn {
    Warmup,
    #[default]
    Active,
    Recovery,
    Cooldown,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum TargetIn {
    Steady { pct: f64 },
    Ramp { from: f64, to: f64 },
}

fn parse_workout(json: &str) -> Result<Workout, String> {
    let w: WorkoutIn = serde_json::from_str(json).map_err(|e| format!("invalid workout: {e}"))?;
    Ok(Workout {
        name: w.name,
        steps: w
            .steps
            .into_iter()
            .map(|s| WorkoutStep {
                duration_s: s.duration_s,
                kind: match s.kind {
                    KindIn::Warmup => StepKind::Warmup,
                    KindIn::Active => StepKind::Active,
                    KindIn::Recovery => StepKind::Recovery,
                    KindIn::Cooldown => StepKind::Cooldown,
                },
                target: match s.target {
                    TargetIn::Steady { pct } => StepTarget::Steady { pct_ftp: pct },
                    TargetIn::Ramp { from, to } => StepTarget::Ramp {
                        from_pct: from,
                        to_pct: to,
                    },
                },
            })
            .collect(),
    })
}

/// Planned duration, NP, IF, TSS and kJ of a workout for `ftp`.
///
/// # Errors
///
/// Invalid workout JSON.
///
/// ```
/// let json = r#"{"name":"FTP hour","steps":[{"duration_s":3600,"target":{"type":"steady","pct":100}}]}"#;
/// let load: serde_json::Value = serde_json::from_str(&zerofit_wasm::plan_workout_json(json, 250.0).unwrap()).unwrap();
/// assert!((load["tss"].as_f64().unwrap() - 100.0).abs() < 1e-9);
/// ```
pub fn plan_workout_json(workout_json: &str, ftp: f64) -> Result<String, String> {
    let load: PlannedLoad = parse_workout(workout_json)?.planned_load(ftp);
    serde_json::to_string(&load).map_err(|e| e.to_string())
}

// ----------------------------------------------------------------- fueling

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FuelIn {
    body_mass_kg: f64,
    #[serde(default)]
    ftp_w: Option<f64>,
    sessions: Vec<SessionIn>,
    /// The next days, tomorrow first (look-ahead uses two).
    #[serde(default)]
    ahead: Vec<AheadIn>,
    #[serde(default)]
    wake_min: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AheadIn {
    #[serde(default)]
    sessions: Vec<SessionIn>,
    #[serde(default)]
    priority: Option<PriorityIn>,
}

#[derive(Deserialize, Clone, Copy)]
enum PriorityIn {
    A,
    B,
    C,
}

impl From<PriorityIn> for Priority {
    fn from(p: PriorityIn) -> Self {
        match p {
            PriorityIn::A => Self::A,
            PriorityIn::B => Self::B,
            PriorityIn::C => Self::C,
        }
    }
}

fn planned_sessions(sessions: &[SessionIn]) -> Vec<PlannedSession> {
    let mut out: Vec<PlannedSession> = sessions
        .iter()
        .map(|s| PlannedSession {
            work_kj: s.work_kj,
            ..PlannedSession::new(s.start_min, s.duration_min, s.intensity_factor)
        })
        .collect();
    out.sort_by_key(|s| s.start_min);
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionIn {
    start_min: u32,
    duration_min: u32,
    intensity_factor: f64,
    #[serde(default)]
    work_kj: Option<f64>,
}

/// A day's fueling plan ([`zerofit_fueling::day_plan`]). Sessions are
/// sorted by start before planning.
///
/// # Errors
///
/// Invalid JSON, a non-positive body mass or overlapping sessions.
pub fn fueling_day_json(input_json: &str) -> Result<String, String> {
    let input: FuelIn =
        serde_json::from_str(input_json).map_err(|e| format!("invalid input: {e}"))?;
    let sessions = planned_sessions(&input.sessions);
    let ahead_sessions: Vec<Vec<PlannedSession>> = input
        .ahead
        .iter()
        .map(|d| planned_sessions(&d.sessions))
        .collect();
    let ahead: Vec<DayAhead<'_>> = input
        .ahead
        .iter()
        .zip(&ahead_sessions)
        .map(|(d, s)| DayAhead {
            sessions: s,
            priority: d.priority.map(Priority::from),
        })
        .collect();
    let mut schedule = MealSchedule::default();
    if let Some(w) = input.wake_min {
        schedule.wake_min = w;
    }
    let plan: DayPlan = zerofit_fueling::day_plan(&DayInput {
        athlete: Athlete {
            body_mass_kg: input.body_mass_kg,
            ftp_w: input.ftp_w,
        },
        sessions: &sessions,
        ahead: &ahead,
        schedule,
    })
    .map_err(|e| e.to_string())?;
    // The plain-English raise reason comes from the crate's `Display`, so
    // the wording lives in one place.
    let mut value = serde_json::to_value(&plan).map_err(|e| e.to_string())?;
    if let (Some(obj), Some(raise)) = (value.as_object_mut(), plan.raise) {
        obj.insert("raise_text".into(), raise.to_string().into());
    }
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

// ----------------------------------------------------------------- exports

// Takes `String` so it can be passed to `map_err` directly.
#[allow(clippy::needless_pass_by_value)]
fn js(e: String) -> wasm_bindgen::JsError {
    wasm_bindgen::JsError::new(&e)
}

/// Decodes and analyzes a FIT file. See [`analyze_activity`].
///
/// # Errors
///
/// As a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn analyze(
    fit_bytes: &[u8],
    settings_json: &str,
) -> Result<ActivityResult, wasm_bindgen::JsError> {
    analyze_activity(fit_bytes, settings_json).map_err(js)
}

/// Summary JSON only. See [`analyze_json`].
///
/// # Errors
///
/// As a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn analyze_summary(
    fit_bytes: &[u8],
    settings_json: &str,
) -> Result<String, wasm_bindgen::JsError> {
    analyze_json(fit_bytes, settings_json).map_err(js)
}

/// Fitness across activities. See [`fitness_json`].
///
/// # Errors
///
/// As a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn fitness(input_json: &str) -> Result<String, wasm_bindgen::JsError> {
    fitness_json(input_json).map_err(js)
}

/// Planned load of a workout. See [`plan_workout_json`].
///
/// # Errors
///
/// As a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn plan_workout(workout_json: &str, ftp: f64) -> Result<String, wasm_bindgen::JsError> {
    plan_workout_json(workout_json, ftp).map_err(js)
}

/// A workout as Zwift `.zwo` XML.
///
/// # Errors
///
/// Invalid workout JSON, as a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn export_zwo(workout_json: &str) -> Result<String, wasm_bindgen::JsError> {
    Ok(parse_workout(workout_json).map_err(js)?.to_zwo())
}

/// A workout as a FIT workout file; `time_created` is FIT time.
///
/// # Errors
///
/// Invalid workout JSON, as a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn export_fit_workout(
    workout_json: &str,
    time_created: u32,
) -> Result<Vec<u8>, wasm_bindgen::JsError> {
    parse_workout(workout_json)
        .map_err(js)?
        .to_fit(time_created)
        .map_err(|e| js(e.to_string()))
}

/// A day's fueling plan. See [`fueling_day_json`].
///
/// # Errors
///
/// As a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn fueling_day(input_json: &str) -> Result<String, wasm_bindgen::JsError> {
    fueling_day_json(input_json).map_err(js)
}

/// The crate version, for diagnostics.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
#[must_use]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Option<Vec<u8>> {
        std::fs::read(format!(
            "{}/../zerofit/tests/fixtures/{name}.fit",
            env!("CARGO_MANIFEST_DIR")
        ))
        .ok()
    }

    #[test]
    fn analyzes_a_fixture_with_streams() {
        let Some(bytes) = fixture("icu_laps") else {
            return;
        };
        let mut a = analyze_activity(&bytes, r#"{"ftp": 323, "lthr": 165}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&a.summary_json()).unwrap();
        assert_eq!(v["sport"], "cycling");
        let np = v["summary"]["normalized_power"].as_f64().unwrap();
        assert!((np - 254.0).abs() < 1.0, "{np}");
        let n = a.take_power().len();
        assert_eq!(n, 4383);
        assert_eq!(a.take_elapsed().len(), n);
        assert_eq!(a.take_heart_rate().len(), n);
        assert_eq!(a.take_w_prime_balance().len(), n);
        let d = a.take_curve_durations();
        assert_eq!(d.len(), a.take_curve_watts().len());
        assert_eq!(d.last(), Some(&4549)); // elapsed-time curve
        assert_eq!(a.take_power(), [] as [u16; 0]); // moved out
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(
            analyze_json(&[], "{")
                .unwrap_err()
                .starts_with("invalid settings")
        );
        assert!(
            analyze_json(&[], r#"{"fpt": 250}"#)
                .unwrap_err()
                .contains("unknown field")
        );
        assert!(
            analyze_json(&[], r#"{"trimp": "x"}"#)
                .unwrap_err()
                .contains("trimp")
        );
        assert!(
            analyze_json(&[], r#"{"power_zones": [0.9, 0.5]}"#)
                .unwrap_err()
                .contains("zones")
        );
    }

    #[test]
    fn fitness_series_and_season_curve() {
        let input = r#"{
            "end_day": 109,
            "activities": [
                {"day": 100, "tss": 80, "curve": [[1, 900], [300, 300], [1200, 260]]},
                {"day": 102, "tss": 120, "curve": [[1, 800], [300, 320], [1200, 250]]},
                {"day": 102, "tss": 30, "curve": []}
            ]
        }"#;
        let v: serde_json::Value = serde_json::from_str(&fitness_json(input).unwrap()).unwrap();
        assert_eq!(v["first_day"], 100);
        let days = v["days"].as_array().unwrap();
        assert_eq!(days.len(), 10);
        assert_eq!(days[2]["load"], 150.0);
        let season = v["season_curve"].as_array().unwrap();
        assert_eq!(season[0], serde_json::json!([1, 900.0]));
        assert_eq!(season[1], serde_json::json!([300, 320.0]));
        assert!(fitness_json("{}").is_err());
        let empty = fitness_json(r#"{"end_day": 5, "activities": []}"#).unwrap();
        assert!(empty.contains("\"days\":[{"));
    }

    #[test]
    fn workout_round_trips() {
        let json = r#"{"name":"VO2","steps":[
            {"duration_s":600,"kind":"warmup","target":{"type":"ramp","from":50,"to":75}},
            {"duration_s":300,"target":{"type":"steady","pct":115}},
            {"duration_s":300,"kind":"recovery","target":{"type":"steady","pct":50}}]}"#;
        let w = parse_workout(json).unwrap();
        assert_eq!(w.steps.len(), 3);
        assert_eq!(w.steps[1].kind, StepKind::Active);
        assert!(w.to_zwo().contains("<Warmup"));
        assert!(w.to_fit(0).unwrap().len() > 50);
        assert!(plan_workout_json(json, 250.0).unwrap().contains("\"tss\""));
        assert!(
            parse_workout(r#"{"name":"x","steps":[{"duration_s":1,"target":{"type":"wobble"}}]}"#)
                .is_err()
        );
    }

    #[test]
    fn fueling_day_plan() {
        let json = r#"{"body_mass_kg": 70, "ftp_w": 250,
            "sessions": [{"start_min": 540, "duration_min": 180, "intensity_factor": 0.75}],
            "ahead": [{"sessions": [{"start_min": 540, "duration_min": 300, "intensity_factor": 0.7}],
                       "priority": "A"}]}"#;
        let v: serde_json::Value = serde_json::from_str(&fueling_day_json(json).unwrap()).unwrap();
        assert_eq!(v["band"], "VeryHigh");
        assert_eq!(v["raise"]["reason"]["type"], "carb_load");
        assert_eq!(v["raise_text"], "Carb-loading: 5 h A event tomorrow");
        assert!(v["own_carbs_g_per_kg"].as_f64().unwrap() < 9.0);
        assert!(v["entries"].as_array().unwrap().len() > 8);
        assert_eq!(v["entries"][0]["kind"]["type"], "pre_session");
        assert_eq!(v["entries"][0]["kind"]["part"], "Meal");
        assert!(fueling_day_json(r#"{"body_mass_kg": 0, "sessions": []}"#).is_err());
    }

    #[test]
    fn grid_edges() {
        assert_eq!(curve_grid(0), [] as [usize; 0]);
        assert_eq!(curve_grid(5), vec![1, 2, 3, 4, 5]);
        assert_eq!(curve_grid(21), {
            let mut v: Vec<usize> = (1..=20).collect();
            v.push(21);
            v
        });
    }

    #[test]
    fn version_is_set() {
        assert_ne!(version(), "");
    }
}
