//! Prints a full activity report for a FIT file.
//!
//! ```sh
//! cargo run -p zerofit-analytics --example ride-report -- ride.fit --ftp 250 \
//!     [--weight 72] [--lthr 165] [--max-hr 188] [--rest-hr 48] \
//!     [--cp 260] [--wprime 20000] [--female] [--recording-time]
//! ```
//!
//! `--recording-time` computes TSS over recording time (TrainingPeaks)
//! instead of moving time (intervals.icu).
#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]

use std::process::ExitCode;

use zerofit_analytics::fit::read_fit;
use zerofit_analytics::summary::LoadDuration;
use zerofit_analytics::{AnalysisConfig, AthleteSettings, TrimpCoefficients, analyze_records};

const USAGE: &str = "usage: ride-report <file.fit> --ftp <W> [--weight <kg>] [--lthr <bpm>] \
                     [--max-hr <bpm>] [--rest-hr <bpm>] [--cp <W>] [--wprime <J>] [--female] \
                     [--recording-time]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut athlete = AthleteSettings::default();
    let mut config = AnalysisConfig::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<f64, String> {
            args.next()
                .ok_or_else(|| format!("{name} needs a value"))?
                .parse::<f64>()
                .map_err(|e| format!("{name}: {e}"))
        };
        match arg.as_str() {
            "--ftp" => athlete.ftp = Some(value("--ftp")?),
            "--weight" => athlete.weight_kg = Some(value("--weight")?),
            "--lthr" => athlete.lthr = Some(value("--lthr")? as u8),
            "--max-hr" => athlete.max_hr = Some(value("--max-hr")? as u8),
            "--rest-hr" => athlete.resting_hr = Some(value("--rest-hr")? as u8),
            "--cp" => athlete.cp = Some(value("--cp")?),
            "--wprime" => athlete.w_prime = Some(value("--wprime")?),
            "--female" => athlete.trimp = TrimpCoefficients::Female,
            "--recording-time" => config.load_duration = LoadDuration::Recording,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => path = Some(other.to_owned()),
        }
    }
    let path = path.ok_or("no FIT file given")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let activity = read_fit(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let analysis = analyze_records(&activity.records, &activity.timer_events, &athlete, &config);
    let s = &analysis.summary;

    println!("Activity report: {path}");
    if let Some(start) = activity.records.first().map(|r| r.timestamp) {
        println!("  start        {}", fit_time_utc(start));
    }
    println!("  sport        {:?}", activity.sport);
    println!(
        "  time         elapsed {}  recording {}  moving {}",
        hms(f64::from(s.elapsed_time_s)),
        hms(s.recording_time_s as f64),
        hms(s.moving_time_s as f64)
    );

    section("Power");
    if s.has_power {
        row("average", s.average_power, "W");
        row("normalized (NP)", s.normalized_power, "W");
        row("max", s.max_power.map(f64::from), "W");
        row("variability index", s.variability_index, "");
        row("work", s.work_kj, "kJ");
        row("avg W/kg", s.average_watts_per_kg, "W/kg");
        if let Some(ftp) = athlete.ftp {
            row("FTP", Some(ftp), "W");
            row("intensity factor", s.intensity_factor, "");
            let basis = match config.load_duration {
                LoadDuration::Moving => "moving time",
                LoadDuration::Recording => "recording time",
            };
            row(&format!("TSS ({basis})"), s.tss, "");
        } else {
            println!("  (pass --ftp for IF, TSS and power zones)");
        }
        row("min W'bal", s.min_w_prime_balance.map(|j| j / 1000.0), "kJ");
    } else {
        println!("  no power data");
    }

    section("Heart rate");
    if s.average_hr.is_some() {
        row("average", s.average_hr, "bpm");
        row("max", s.max_hr.map(f64::from), "bpm");
        row("hrTSS", s.hr_tss, "");
        row("efficiency factor", s.efficiency_factor, "W/bpm");
        row("Pa:HR decoupling", s.decoupling_pct, "%");
        if athlete.lthr.is_none() {
            println!("  (pass --lthr, --max-hr and --rest-hr for HR zones and hrTSS)");
        }
    } else {
        println!("  no heart-rate data");
    }

    if let Some(zones) = &s.power_zone_seconds {
        section("Time in power zones (Coggan, % FTP)");
        zone_bars(
            zones,
            &[
                "Z1 ≤55",
                "Z2 ≤75",
                "Z3 ≤90",
                "Z4 ≤105",
                "Z5 ≤120",
                "Z6 ≤150",
                "Z7 >150",
            ],
        );
    }
    if let Some(zones) = &s.hr_zone_seconds {
        section("Time in HR zones (Friel, % LTHR)");
        zone_bars(
            zones,
            &[
                "Z1 ≤81",
                "Z2 ≤89",
                "Z3 ≤93",
                "Z4 ≤99",
                "Z5a ≤102",
                "Z5b ≤106",
                "Z5c >106",
            ],
        );
    }

    if !s.power_curve.is_empty() {
        section("Power curve (best average power)");
        for p in &s.power_curve {
            println!("  {:>9}  {:>6.0} W", hms(p.duration_s as f64), p.watts);
        }
    }
    if let Some(fit) = s.cp_fit {
        section("Critical power (2-parameter fit, 2-20 min)");
        println!(
            "  CP {:.0} ± {:.0} W   W' {:.1} ± {:.1} kJ   R² {:.3}   RMSE {:.1} W   ({} points)",
            fit.cp,
            fit.se_cp,
            fit.w_prime / 1000.0,
            fit.se_w_prime / 1000.0,
            fit.r_squared,
            fit.rmse,
            fit.points
        );
        println!("  (one ride is rarely maximal at every duration; trust a season curve more)");
    }
    if let Some(e) = s.eftp {
        println!(
            "  eFTP {:.0} W  from {:.0} W for {}  (W' {:.1} kJ)",
            e.eftp,
            e.watts,
            hms(e.duration_s as f64),
            e.w_prime / 1000.0
        );
    }

    if let Some(r) = s.resample_report {
        section("Resampling");
        println!(
            "  {} records, {} duplicates, {} out of order, {} gaps filled ({} s), \
             {} pauses ({} s), {} power dropouts repaired, {} s without power, {} spikes",
            r.records,
            r.duplicates,
            r.out_of_order,
            r.gaps_filled,
            r.gap_seconds_filled,
            r.pauses,
            r.paused_seconds,
            r.power_dropouts_repaired,
            r.power_dropout_seconds,
            r.power_spikes
        );
    }
    if let Some(dev) = activity.session {
        section("Values stored in the file's session message (for comparison)");
        println!(
            "  avg {:?} W, NP {:?} W, IF {:?}, TSS {:?}, threshold {:?} W, timer {:?} s",
            dev.avg_power,
            dev.normalized_power,
            dev.intensity_factor,
            dev.training_stress_score,
            dev.threshold_power,
            dev.total_timer_time
        );
    }
    Ok(())
}

fn section(title: &str) {
    println!("\n{title}");
}

fn row(label: &str, value: Option<f64>, unit: &str) {
    match value {
        Some(v) => println!("  {label:<22} {v:>9.2} {unit}"),
        None => println!("  {label:<22} {:>9} {unit}", "—"),
    }
}

fn zone_bars(seconds: &[u32], labels: &[&str]) {
    let total: u32 = seconds.iter().sum();
    for (i, &secs) in seconds.iter().enumerate() {
        let pct = if total == 0 {
            0.0
        } else {
            f64::from(secs) / f64::from(total) * 100.0
        };
        let label = labels.get(i).copied().unwrap_or("");
        let bar = "█".repeat((pct / 2.0).round() as usize);
        println!("  {label:<9} {:>9} {pct:>5.1}% {bar}", hms(f64::from(secs)));
    }
}

fn hms(seconds: f64) -> String {
    let s = seconds.round() as u64;
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

/// FIT time (s since 1989-12-31T00:00:00Z) as `YYYY-MM-DD hh:mm:ss UTC`.
fn fit_time_utc(fit: u32) -> String {
    let unix = i64::from(fit) + 631_065_600;
    let (days, secs) = (unix.div_euclid(86_400), unix.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}
