// Pure formatting. Times are absolute and UTC, as every page of the console says them (live-design.md, CON-806).

const two = (n: number) => String(n).padStart(2, "0");

/** `14:32:07` (UTC). */
export function clockUtc(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
}

/** `14:32:07.904` (UTC): the time of one call among many in the same second. */
export function clockMsUtc(ms: number): string {
  return `${clockUtc(ms)}.${String(new Date(ms).getUTCMilliseconds()).padStart(3, "0")}`;
}

/** `2026-11-20 14:32:07 UTC`: a moment said whole, where a clock time alone would not say the day. */
export function dateTimeUtc(ms: number): string {
  const d = new Date(ms);
  return `${d.getUTCFullYear()}-${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${clockUtc(ms)} UTC`;
}

/** How long something took, from microseconds: `412 µs`, `2.4 ms`, `19 ms`, `1.25 s`. Three figures at most, so a column of
 * them reads at a glance. */
export function micros(us: number): string {
  const n = Math.max(0, us);
  if (n < 1_000) return `${Math.round(n)} µs`;
  if (n < 10_000) return `${(n / 1_000).toFixed(1)} ms`;
  if (n < 1_000_000) return `${Math.round(n / 1_000)} ms`;
  return `${(n / 1_000_000).toFixed(2)} s`;
}

/** A size in binary units: `512 B`, `1.5 KiB`, `12 MiB`. */
export function bytes(n: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let v = Math.max(0, n);
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u += 1;
  }
  return `${u === 0 || v >= 10 ? Math.round(v) : v.toFixed(1)} ${units[u]}`;
}

/** `14:32` (UTC). */
export function hhmmUtc(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}`;
}

/** `12,345`: grouped with commas whatever the browser's locale, as the server's `number` does. */
export function number(n: number): string {
  const neg = n < 0;
  const digits = String(Math.trunc(Math.abs(n)));
  let out = "";
  for (let i = 0; i < digits.length; i++) {
    if (i > 0 && (digits.length - i) % 3 === 0) out += ",";
    out += digits[i];
  }
  return neg ? `-${out}` : out;
}

/** `3 d 4 h`, `2 h 5 min`, `42 s`: how long, for a tooltip (never updated in place). */
export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return h > 0 ? `${d} d ${h} h` : `${d} d`;
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  if (m > 0) return `${m} min`;
  return `${s} s`;
}

/** The plural of a noun: `topic` is `topics`, `policy` is `policies`, `bus` is `buses`, `batch` is `batches`; a noun of several words is
 * pluralised at its last (`log group` is `log groups`). */
export function pluralWord(one: string): string {
  if (/[^aeiou]y$/i.test(one)) return `${one.slice(0, -1)}ies`;
  if (/(s|x|z|ch|sh)$/i.test(one)) return `${one}es`;
  return `${one}s`;
}

/** `1 topic`, `3 topics`. */
export function plural(n: number, one: string, many = pluralWord(one)): string {
  return `${number(n)} ${n === 1 ? one : many}`;
}
