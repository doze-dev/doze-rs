// The time range a log search or a stream's events cover, and the zone they are said in: pure, so every state is testable.

/** The relative ranges, back from now. */
export const RANGES: { id: string; label: string; ms: number | null }[] = [
  { id: "5m", label: "Last 5 minutes", ms: 5 * 60_000 },
  { id: "15m", label: "Last 15 minutes", ms: 15 * 60_000 },
  { id: "1h", label: "Last 1 hour", ms: 3_600_000 },
  { id: "6h", label: "Last 6 hours", ms: 6 * 3_600_000 },
  { id: "24h", label: "Last 24 hours", ms: 24 * 3_600_000 },
  { id: "7d", label: "Last 7 days", ms: 7 * 24 * 3_600_000 },
  { id: "all", label: "All time", ms: null },
];

export type Range = { mode: "relative"; id: string } | { mode: "absolute"; from: number | null; to: number | null };
export const DEFAULT_RANGE: Range = { mode: "relative", id: "1h" };
export type Zone = "utc" | "local";

/** FilterLogEvents' start and end for a range, at `now`. */
export function bounds(r: Range, now: number): { startTime?: number; endTime?: number } {
  if (r.mode === "absolute") return { ...(r.from !== null ? { startTime: r.from } : {}), ...(r.to !== null ? { endTime: r.to } : {}) };
  const span = RANGES.find((x) => x.id === r.id)?.ms ?? null;
  return span === null ? {} : { startTime: now - span, endTime: now };
}

/** `Last 1 hour`, `All time`, or the two ends. */
export function rangeWords(r: Range, zone: Zone = "utc"): string {
  if (r.mode === "relative") return RANGES.find((x) => x.id === r.id)?.label ?? "All time";
  if (r.from === null && r.to === null) return "All time";
  return `${r.from === null ? "the start" : minutes(r.from, zone)} to ${r.to === null ? "now" : minutes(r.to, zone)}`;
}

/** `the last hour`, `the last 15 minutes`, `the whole group`: a range as a sentence says it. */
export function spanWords(r: Range): string {
  if (r.mode === "absolute") return rangeWords(r);
  if (r.id === "1h") return "the last hour";
  const w = RANGES.find((x) => x.id === r.id);
  return !w || w.ms === null ? "the whole group" : w.label.toLowerCase().replace(/^last /, "the last ");
}

const two = (n: number) => String(n).padStart(2, "0");
/** The fields of a moment in the zone. */
function parts(ms: number, zone: Zone) {
  const d = new Date(ms);
  return zone === "utc"
    ? { y: d.getUTCFullYear(), mo: d.getUTCMonth() + 1, d: d.getUTCDate(), h: d.getUTCHours(), mi: d.getUTCMinutes(), s: d.getUTCSeconds(), ms: d.getUTCMilliseconds() }
    : { y: d.getFullYear(), mo: d.getMonth() + 1, d: d.getDate(), h: d.getHours(), mi: d.getMinutes(), s: d.getSeconds(), ms: d.getMilliseconds() };
}

/** `08:13:58.211`. */
export function clock(ms: number, zone: Zone): string {
  const p = parts(ms, zone);
  return `${two(p.h)}:${two(p.mi)}:${two(p.s)}.${String(p.ms).padStart(3, "0")}`;
}

/** `2026-11-08`. */
export function day(ms: number, zone: Zone): string {
  const p = parts(ms, zone);
  return `${p.y}-${two(p.mo)}-${two(p.d)}`;
}

/** `2026-11-08 07:14`. */
export function minutes(ms: number, zone: Zone): string {
  const p = parts(ms, zone);
  return `${day(ms, zone)} ${two(p.h)}:${two(p.mi)}`;
}

/** `5 events, 2026-11-08 07:14 to 08:14 local time`: what the list holds and the span it was asked for (the end said with its day only
 * when it is another day). */
export function spanLine(events: number, from: number, to: number, zone: Zone): string {
  const end = day(from, zone) === day(to, zone) ? minutes(to, zone).slice(11) : minutes(to, zone);
  return `${events.toLocaleString("en-US")} ${events === 1 ? "event" : "events"}, ${minutes(from, zone)} to ${end} ${zone === "utc" ? "UTC" : "local time"}`;
}
