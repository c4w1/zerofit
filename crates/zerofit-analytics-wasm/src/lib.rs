//! WebAssembly wrapper for `zerofit-analytics`: one exported function that
//! takes FIT bytes and athlete settings (JSON) and returns the activity
//! summary as JSON.
//!
//! ```sh
//! cargo build -p zerofit-analytics-wasm --release --target wasm32-unknown-unknown
//! wasm-bindgen --target nodejs --out-dir target/wasm-pkg \
//!     target/wasm32-unknown-unknown/release/zerofit_analytics_wasm.wasm
//! node crates/zerofit-analytics-wasm/tests/smoke.mjs
//! ```
//!
//! From JavaScript:
//!
//! ```js
//! const { analyze } = require("./zerofit_analytics_wasm.js");
//! const summary = JSON.parse(analyze(fitBytes, JSON.stringify({ ftp: 250 })));
//! console.log(summary.summary.normalized_power);
//! ```
//!
//! The logic lives in [`analyze_json`], a plain Rust function, so it is
//! unit-tested natively; the `#[wasm_bindgen]` export only converts the
//! error type.

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::wasm_bindgen;
use zerofit_analytics::fit::{Sport, read_fit};
use zerofit_analytics::summary::ActivitySummary;
use zerofit_analytics::{AnalysisConfig, AthleteSettings, TrimpCoefficients, analyze_records};

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
}

impl Settings {
    fn to_athlete(&self) -> Result<AthleteSettings, String> {
        let trimp = match self.trimp.as_deref() {
            None | Some("male") => TrimpCoefficients::Male,
            Some("female") => TrimpCoefficients::Female,
            Some(other) => return Err(format!("unknown trimp coefficients {other:?}")),
        };
        Ok(AthleteSettings {
            ftp: self.ftp,
            weight_kg: self.weight_kg,
            lthr: self.lthr,
            max_hr: self.max_hr,
            resting_hr: self.resting_hr,
            trimp,
            cp: self.cp,
            w_prime: self.w_prime,
            ..AthleteSettings::default()
        })
    }
}

/// The JSON returned by [`analyze_json`].
#[derive(Debug, Serialize)]
pub struct Output {
    /// `"cycling"`, `"running"`, `"other"` or `null`.
    pub sport: Option<&'static str>,
    /// Every metric.
    pub summary: ActivitySummary,
}

/// Decodes `fit_bytes`, analyzes the activity with the settings in
/// `settings_json` (an object with the [`Settings`] fields; `""` means
/// none) and returns [`Output`] as JSON.
///
/// # Errors
///
/// A message for invalid settings JSON or a FIT decoding error (with the
/// byte offset).
///
/// ```
/// let err = zerofit_analytics_wasm::analyze_json(b"not a fit file", "").unwrap_err();
/// assert!(err.starts_with("FIT decoding failed"));
/// ```
pub fn analyze_json(fit_bytes: &[u8], settings_json: &str) -> Result<String, String> {
    let settings: Settings = if settings_json.trim().is_empty() {
        Settings::default()
    } else {
        serde_json::from_str(settings_json).map_err(|e| format!("invalid settings: {e}"))?
    };
    let athlete = settings.to_athlete()?;
    let activity = read_fit(fit_bytes).map_err(|e| format!("FIT decoding failed: {e}"))?;
    let analysis = analyze_records(
        &activity.records,
        &activity.timer_events,
        &athlete,
        &AnalysisConfig::default(),
    );
    let output = Output {
        sport: activity.sport.map(|s| match s {
            Sport::Cycling => "cycling",
            Sport::Running => "running",
            Sport::Other => "other",
        }),
        summary: analysis.summary,
    };
    serde_json::to_string(&output).map_err(|e| format!("serialization failed: {e}"))
}

/// JavaScript entry point: [`analyze_json`], throwing an `Error` on failure.
///
/// # Errors
///
/// As [`analyze_json`], as a JavaScript `Error`.
#[allow(unsafe_code)] // wasm-bindgen's generated FFI glue
#[wasm_bindgen]
pub fn analyze(fit_bytes: &[u8], settings_json: &str) -> Result<String, wasm_bindgen::JsError> {
    analyze_json(fit_bytes, settings_json).map_err(|e| wasm_bindgen::JsError::new(&e))
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
    fn analyzes_a_fixture() {
        let Some(bytes) = fixture("icu_laps") else {
            return;
        };
        let json = analyze_json(&bytes, r#"{"ftp": 323, "lthr": 165}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["sport"], "cycling");
        let np = v["summary"]["normalized_power"].as_f64().unwrap();
        assert!((np - 254.0).abs() < 1.0, "{np}");
        assert!(v["summary"]["power_curve"].as_array().unwrap().len() > 10);
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
    }

    #[test]
    fn version_is_set() {
        assert!(!version().is_empty());
    }
}
