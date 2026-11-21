// EventBridge's schedule expressions, read in the browser: `cron(minutes hours day-of-month month day-of-week year)` and
// `rate(value unit)`. The next times a cron fires (in UTC), its plain-English reading, and the problems with a rate. Pure.
//
// What is understood: `*`, `?`, numbers, `a-b`, `a,b`, `a/n`, `*/n`, month and weekday names. `L`, `W` and `#` are valid on AWS and are
// not worked out here: the page then says it cannot list the dates, and the service still takes the expression.

export type CronFields = { minutes: string; hours: string; dom: string; month: string; dow: string; year: string };
export const CRON_PARTS: (keyof CronFields)[] = ["minutes", "hours", "dom", "month", "dow", "year"];

/** The six fields of a `cron(...)` expression, or `null` when it is not one. */
export function parseCron(expression: string): CronFields | null {
  const m = /^cron\((.*)\)$/.exec(expression.trim());
  const parts = m?.[1]?.trim().split(/\s+/) ?? [];
  if (parts.length !== 6) return null;
  const [minutes, hours, dom, month, dow, year] = parts as [string, string, string, string, string, string];
  return { minutes, hours, dom, month, dow, year };
}

export const cronText = (f: CronFields): string => `cron(${CRON_PARTS.map((k) => f[k]).join(" ")})`;

const MONTHS = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const DAYS = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
const MONTH_NAMES = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

type Range = { lo: number; hi: number; names?: string[]; offset?: number };
const RANGES: Record<keyof CronFields, Range> = {
  minutes: { lo: 0, hi: 59 },
  hours: { lo: 0, hi: 23 },
  dom: { lo: 1, hi: 31 },
  month: { lo: 1, hi: 12, names: MONTHS, offset: 1 },
  // AWS numbers the week from Sunday = 1.
  dow: { lo: 1, hi: 7, names: DAYS, offset: 1 },
  year: { lo: 1970, hi: 2199 },
};

const num = (token: string, r: Range): number | null => {
  const named = r.names?.indexOf(token.toUpperCase()) ?? -1;
  if (named >= 0) return named + (r.offset ?? 0);
  return /^\d+$/.test(token) ? Number(token) : null;
};

/** The values a field allows, or `null` when it holds something this module does not work out (`L`, `W`, `#`) or is invalid. `*` and `?` are every value. */
export function values(field: string, which: keyof CronFields): number[] | null {
  const r = RANGES[which];
  const all = Array.from({ length: r.hi - r.lo + 1 }, (_, i) => r.lo + i);
  if (field === "*" || field === "?") return all;
  const out = new Set<number>();
  for (const part of field.split(",")) {
    const [span = "", stepText] = part.split("/");
    const step = stepText === undefined ? 1 : Number(stepText);
    if (!Number.isInteger(step) || step < 1) return null;
    let lo: number | null;
    let hi: number | null;
    if (span === "*") [lo, hi] = [r.lo, r.hi];
    else if (span.includes("-")) {
      const [a = "", b = ""] = span.split("-");
      [lo, hi] = [num(a, r), num(b, r)];
    } else {
      lo = num(span, r);
      hi = stepText === undefined ? lo : r.hi;
    }
    if (lo === null || hi === null || lo < r.lo || hi > r.hi || lo > hi) return null;
    for (let v = lo; v <= hi; v += step) out.add(v);
  }
  return [...out].sort((a, b) => a - b);
}

/** Why `fields` cannot be a schedule, in the service's terms; `null` when they can. */
export function cronProblem(f: CronFields): string | null {
  if (f.dom !== "?" && f.dow !== "?") return "Day of month and day of week can't both have a value: put ? in one.";
  if (f.dom === "?" && f.dow === "?") return "One of day of month and day of week needs a value.";
  for (const k of CRON_PARTS) {
    if (f[k] === "") return `${k === "dom" ? "Day of month" : k === "dow" ? "Day of week" : k.charAt(0).toUpperCase() + k.slice(1)} is empty.`;
  }
  return null;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** The next `n` times (UTC milliseconds) `f` fires after `from`; `null` when a field uses what this module does not work out. */
export function nextRuns(f: CronFields, from: number, n = 10): number[] | null {
  const [mi, ho, dm, mo, dw, yr] = [values(f.minutes, "minutes"), values(f.hours, "hours"), values(f.dom, "dom"), values(f.month, "month"), values(f.dow, "dow"), values(f.year, "year")];
  if (!mi || !ho || !dm || !mo || !dw || !yr) return null;
  const out: number[] = [];
  const day = new Date(from);
  day.setUTCHours(0, 0, 0, 0);
  // Five years is further than any listing needs; a schedule that never fires (31 February) ends the search there.
  for (let i = 0; i < 366 * 5 && out.length < n; i++, day.setUTCDate(day.getUTCDate() + 1)) {
    if (!yr.includes(day.getUTCFullYear()) || !mo.includes(day.getUTCMonth() + 1)) continue;
    const byDate = f.dom !== "?" && f.dom !== "*" ? dm.includes(day.getUTCDate()) : true;
    const byWeek = f.dow !== "?" && f.dow !== "*" ? dw.includes(day.getUTCDay() + 1) : true;
    if (!byDate || !byWeek) continue;
    for (const h of ho) for (const m of mi) {
      const at = Date.UTC(day.getUTCFullYear(), day.getUTCMonth(), day.getUTCDate(), h, m);
      if (at > from && out.length < n) out.push(at);
    }
  }
  return out;
}

const clock = (h: number, m: number) => `${h % 12 === 0 ? 12 : h % 12}:${pad(m)} ${h < 12 ? "am" : "pm"}`;

function dayWords(f: CronFields): string {
  if (f.dom === "?" || f.dom === "*") {
    if (f.dow === "?" || f.dow === "*") return "every day";
    const v = values(f.dow, "dow");
    if (!v) return `on days of the week ${f.dow}`;
    const names = v.map((d) => DAY_NAMES[d - 1] ?? String(d));
    if (v.length > 2 && v[v.length - 1]! - v[0]! === v.length - 1) return `every ${names[0]} through ${names[names.length - 1]}`;
    return `every ${names.join(", ")}`;
  }
  const v = values(f.dom, "dom");
  return v && v.length === 1 ? `on day ${v[0]} of the month` : `on days ${f.dom} of the month`;
}

/** The expression as a sentence: `6:00 pm UTC every Monday through Friday`. */
export function cronWords(f: CronFields): string {
  const mi = values(f.minutes, "minutes");
  const ho = values(f.hours, "hours");
  let time: string;
  if (mi && ho && mi.length === 1 && ho.length === 1) time = `${clock(ho[0]!, mi[0]!)} UTC`;
  else if (f.minutes === "*" && f.hours === "*") time = "every minute";
  else if (/^\*\/\d+$/.test(f.minutes) && f.hours === "*") time = `every ${f.minutes.slice(2)} minutes`;
  else if (mi && mi.length === 1 && f.hours === "*") time = `at minute ${mi[0]} of every hour`;
  else if (mi && ho && mi.length === 1) time = `at ${ho.map((h) => clock(h, mi[0]!)).join(", ")} UTC`;
  else time = `at minute ${f.minutes} of hour ${f.hours} UTC`;
  const months = f.month === "*" ? "" : ` in ${(values(f.month, "month") ?? []).map((m) => MONTH_NAMES[m - 1] ?? m).join(", ")}`;
  const year = f.year === "*" ? "" : ` in ${f.year}`;
  const days = dayWords(f);
  return `${time}${days === "every day" && time.startsWith("every") ? "" : ` ${days}`}${months}${year}`;
}

export type Rate = { value: number; unit: "minute" | "hour" | "day" };

/** A `rate(value unit)` expression, or why it is not one. */
export function parseRate(expression: string): { rate: Rate } | { problem: string } {
  const m = /^rate\((.*)\)$/.exec(expression.trim());
  if (!m) return { problem: "A rate expression is rate(value unit)." };
  const [v = "", u = "", extra] = m[1]!.trim().split(/\s+/);
  const value = Number(v);
  if (!/^\d+$/.test(v) || value < 1) return { problem: "The value of a rate is a positive whole number." };
  const unit = /^(minute|hour|day)s?$/.exec(u)?.[1] as Rate["unit"] | undefined;
  if (!unit || extra !== undefined) return { problem: "The unit of a rate is minute, hour or day." };
  if ((value === 1) !== (u === unit)) return { problem: value === 1 ? `The unit is singular for 1: rate(1 ${unit}).` : `The unit is plural above 1: rate(${value} ${unit}s).` };
  return { rate: { value, unit } };
}

/** `Mon, Nov 9 18:00 UTC`. */
export function runWords(ms: number): string {
  const d = new Date(ms);
  return `${d.toLocaleString("en-US", { weekday: "short", timeZone: "UTC" })}, ${d.toLocaleString("en-US", { month: "short", timeZone: "UTC" })} ${d.getUTCDate()} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())} UTC`;
}

/** The same moment in the browser's own time zone, with the zone's name. */
export function localRunWords(ms: number): string {
  const d = new Date(ms);
  const zone = new Intl.DateTimeFormat("en-US", { timeZoneName: "short" }).formatToParts(d).find((p) => p.type === "timeZoneName")?.value ?? "";
  return `${d.toLocaleString("en-US", { weekday: "short" })}, ${d.toLocaleString("en-US", { month: "short" })} ${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())} ${zone}`;
}
