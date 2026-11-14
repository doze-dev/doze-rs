// Pure formatting. Times are absolute and UTC, as every page of the console says them (live-design.md, CON-806).

const two = (n: number) => String(n).padStart(2, "0");

/** `14:32:07` (UTC). */
export function clockUtc(ms: number): string {
  const d = new Date(ms);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}:${two(d.getUTCSeconds())}`;
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

/** `1 topic`, `3 topics`. */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${number(n)} ${n === 1 ? one : many}`;
}
