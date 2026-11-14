// The server's formatting (crates/kafka-console/src/format.rs) in TypeScript, so a page says sizes, spans and times exactly as
// the classic console does. Pure; UTC.
import { number } from "./format";

const MIN = 60;
const HOUR = 3600;
const DAY = 86_400;
const two = (n: number) => String(n).padStart(2, "0");
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** `41 s`, `2 min`, `3 h 5 min`, `3 d 4 h`: how long, in the two largest units. Negative is zero. */
export function span(secs: number): string {
  const s = Math.max(0, Math.trunc(secs));
  const pair = (big: number, bu: string, rest: number, su: string) => (rest === 0 ? `${big} ${bu}` : `${big} ${bu} ${rest} ${su}`);
  if (s < MIN) return `${s} s`;
  if (s < HOUR) return `${Math.floor(s / MIN)} min`;
  if (s < DAY) return pair(Math.floor(s / HOUR), "h", Math.floor((s % HOUR) / MIN), "min");
  return pair(Math.floor(s / DAY), "d", Math.floor((s % DAY) / HOUR), "h");
}

/** {@link span} of milliseconds. */
export function spanMs(ms: number): string {
  return span(Math.trunc(ms / 1000));
}

/** `512 B`, `7.9 MiB`: tenths rounded down, as the server does. */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let unit = 0;
  while (unit + 1 < units.length && n >= 1024 ** (unit + 2)) unit++;
  const size = 1024 ** (unit + 1);
  const whole = Math.floor(n / size);
  const tenths = Math.floor(((n - whole * size) * 10) / size);
  return `${whole}.${tenths} ${units[unit]}`;
}

/** `2 ms`, `1.4 s`. */
export function millis(ms: number): string {
  return ms < 1000 ? `${ms} ms` : `${Math.floor(ms / 1000)}.${Math.floor((ms % 1000) / 100)} s`;
}

/** `22:13:33` (UTC). */
export function clock(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
}

/** `22:13:33.208` (UTC). */
export function clockMs(ms: number): string {
  return `${clock(ms)}.${String(((ms % 1000) + 1000) % 1000).padStart(3, "0")}`;
}

/** `Nov 6 18:02 UTC`. */
export function dateTime(ms: number): string {
  const d = new Date(ms);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())} UTC`;
}

/** The time of day when it is today, else the date and time (no zone: the column says UTC). */
export function stamp(ms: number, now: number): string {
  return Math.floor(ms / (DAY * 1000)) === Math.floor(now / (DAY * 1000)) ? clock(ms) : dateTime(ms).replace(/ UTC$/, "");
}

/** `7 d`, or `for ever` for -1. */
export function retention(ms: number): string {
  return ms < 0 ? "for ever" : spanMs(ms);
}

/** A transaction's timeout: whole seconds up to two minutes (`60 s`), else a span. */
export function timeoutText(ms: number): string {
  return ms > 0 && ms % 1000 === 0 && ms <= 120_000 ? `${ms / 1000} s` : spanMs(ms);
}

/** `a, b and c`. */
export function listAnd(items: (string | number)[]): string {
  const s = items.map(String);
  if (s.length <= 1) return s.join("");
  return `${s.slice(0, -1).join(", ")} and ${s[s.length - 1]}`;
}

/** `1 record`, `3 records`. */
export function count(n: number, one: string, many: string): string {
  return `${number(n)} ${n === 1 ? one : many}`;
}

/** `61 %`. */
export function pct(share: number): string {
  return `${Math.round(share * 100)} %`;
}
