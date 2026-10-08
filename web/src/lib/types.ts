// Types mirroring the JSON produced by crates/zerofit-wasm (serde output of
// the Rust structs). Field names are the Rust field names.

export interface CpFit {
  cp: number;
  w_prime: number;
  k: number | null;
  p_max: number | null;
  r_squared: number;
  rmse: number;
  se_cp: number;
  se_w_prime: number;
  points: number;
}

export interface EftpEstimate {
  eftp: number;
  duration_s: number;
  watts: number;
  w_prime: number;
}

export interface ResampleReport {
  records: number;
  duplicates: number;
  out_of_order: number;
  timer_paused_records: number;
  power_spikes: number;
  gaps_filled: number;
  gap_seconds_filled: number;
  pauses: number;
  paused_seconds: number;
  power_dropouts_repaired: number;
  power_dropout_seconds: number;
  hr_dropouts_repaired: number;
}

export interface ActivitySummary {
  recording_time_s: number;
  elapsed_time_s: number;
  moving_time_s: number;
  has_power: boolean;
  average_power: number | null;
  max_power: number | null;
  normalized_power: number | null;
  intensity_factor: number | null;
  tss: number | null;
  variability_index: number | null;
  work_kj: number | null;
  average_watts_per_kg: number | null;
  average_hr: number | null;
  max_hr: number | null;
  hr_tss: number | null;
  efficiency_factor: number | null;
  decoupling_pct: number | null;
  power_zone_seconds: number[] | null;
  hr_zone_seconds: number[] | null;
  power_curve: { duration_s: number; watts: number }[];
  cp_fit: CpFit | null;
  eftp: EftpEstimate | null;
  min_w_prime_balance: number | null;
  resample_report: ResampleReport | null;
}

/** Per-second streams for the charts (typed arrays transferred from the worker). */
export interface Streams {
  elapsed: Uint32Array;
  power: Uint16Array;
  heartRate: Uint8Array;
  cadence: Uint8Array;
  altitude: Float32Array;
  speed: Float32Array;
  wPrimeBalance: Float32Array;
}

export interface AnalyzedActivity {
  sport: string | null;
  summary: ActivitySummary;
  /** FIT time of the first sample (s since 1989-12-31 UTC). */
  startTime: number;
  curveDurations: Uint32Array;
  curveWatts: Float32Array;
  /** Wall-clock time the worker spent decoding and analyzing, ms. */
  analysisMs: number;
}

export interface DailyLoad {
  load: number;
  ctl: number;
  atl: number;
  tsb: number;
}

export interface Fitness {
  first_day: number;
  days: DailyLoad[];
  season_curve: [number, number][];
  cp_2p: CpFit | null;
  cp_3p: CpFit | null;
  eftp: EftpEstimate | null;
}

export type StepKind = "warmup" | "active" | "recovery" | "cooldown";
export type StepTarget = { type: "steady"; pct: number } | { type: "ramp"; from: number; to: number };

export interface WorkoutStep {
  duration_s: number;
  kind: StepKind;
  target: StepTarget;
}

export interface Workout {
  name: string;
  steps: WorkoutStep[];
}

export interface PlannedLoad {
  duration_s: number;
  average_power: number;
  normalized_power: number | null;
  intensity_factor: number | null;
  tss: number | null;
  work_kj: number;
}

export type LoadBand = "Light" | "Moderate" | "High" | "VeryHigh";

export type EntryKind =
  | { type: "meal"; meal: "Breakfast" | "Lunch" | "Dinner" | "EveningSnack" }
  | { type: "pre_session"; session: number }
  | { type: "during_session"; session: number }
  | { type: "recovery"; session: number; hour: number };

export interface FuelEntry {
  time_min: number;
  kind: EntryKind;
  carbs_g: number;
  protein_g: number;
}

export interface SessionPlan {
  start_min: number;
  duration_min: number;
  load_kj_per_kg: number;
  pre: { hours_before: number; carbs_g_per_kg: number };
  during: { carbs_g_per_hour: number; multiple_transportable: boolean; feed_interval_min: number };
  during_total_g: number;
  recovery: { carbs_g_per_kg_per_hour: number; hours: number; protein_g_per_kg: number };
}

export interface DayPlan {
  load_kj_per_kg: number;
  band: LoadBand;
  carbs_g_per_kg: number;
  carbs_target_g: number;
  carbs_planned_g: number;
  protein_g_per_kg: number;
  protein_g: number;
  session_feeds_exceed_target: boolean;
  sessions: SessionPlan[];
  entries: FuelEntry[];
}

export interface FuelSessionIn {
  start_min: number;
  duration_min: number;
  intensity_factor: number;
  work_kj?: number;
}

export interface FuelDayIn {
  body_mass_kg: number;
  ftp_w?: number;
  sessions: FuelSessionIn[];
  next_session_start_min?: number;
  wake_min?: number;
}

/** Athlete settings, as sent to the WASM module (see `Settings` in zerofit-wasm). */
export interface AthleteSettings {
  ftp: number;
  weight_kg: number;
  lthr: number;
  max_hr: number;
  resting_hr: number;
  trimp: "male" | "female";
  power_zones: number[];
  hr_zones: number[];
  w_prime?: number;
  cp?: number;
}
