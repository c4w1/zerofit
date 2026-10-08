// Formatting and date helpers.

/** Seconds between the FIT epoch (1989-12-31T00:00:00Z) and the Unix epoch. */
export const FIT_EPOCH_OFFSET_S = 631_065_600;

export const fitTimeToMs = (fit: number): number => (fit + FIT_EPOCH_OFFSET_S) * 1000;

/** `h:mm:ss`, or `m:ss` under an hour. */
export function duration(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "—";
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
}

/** A short duration label for curve axes: 5s, 1m, 20m, 1h. */
export function durationLabel(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.round(seconds / 6) / 10}m`.replace(".0m", "m");
  return `${Math.round(seconds / 360) / 10}h`.replace(".0h", "h");
}

export function num(v: number | null | undefined, digits = 0): string {
  if (v == null || !Number.isFinite(v)) return "—";
  return v.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

export function date(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export function dateTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Minutes after midnight as `HH:MM` (wrapping past midnight, marked +1). */
export function clock(minutes: number): string {
  const day = Math.floor(minutes / 1440);
  const m = ((minutes % 1440) + 1440) % 1440;
  const t = `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
  return day > 0 ? `${t} (+${day}d)` : t;
}

/** Local calendar day as days since 1970-01-01 (for CTL/ATL day indices). */
export function dayIndex(ms: number): number {
  const d = new Date(ms);
  return Math.floor(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / 86_400_000);
}

export function dayIndexToDate(day: number): Date {
  const utc = new Date(day * 86_400_000);
  return new Date(utc.getUTCFullYear(), utc.getUTCMonth(), utc.getUTCDate());
}

/** `YYYY-MM-DD` of a local date. */
export function isoDay(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export function parseIsoDay(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}

/** Monday of the week containing `d`. */
export function mondayOf(d: Date): Date {
  const m = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  m.setDate(m.getDate() - ((m.getDay() + 6) % 7));
  return m;
}

export function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

export const weekdayShort = (d: Date) => d.toLocaleDateString(undefined, { weekday: "short" });

/** Triggers a download of `data` as `filename`. */
export function download(filename: string, data: BlobPart, type: string): void {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
