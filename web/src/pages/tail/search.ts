// The Live tail page's address (CON-1580): what it follows, from where, and its filters, the names the server reads
// (`/api/v1/tail/topics`), plus the record Sheet. Each parser keeps a value only when the page can use it, so a hand-edited
// address is a page with fewer choices, never an error.
import { oneOf, shortText } from "../../lib/search";

export type TailSearch = {
  /** Topics by name. */
  topic?: string[];
  /** Glob patterns of names (`orders.*`). */
  pattern?: string[];
  /** Start: from now (default) or the last N per partition. */
  from?: "last";
  last?: number;
  partition?: number[];
  key?: string;
  value?: string;
  header?: string;
  header_value?: string;
  field?: string;
  equals?: string;
  isolation?: "read_committed";
  /** The record Sheet: `topic:partition:offset`. */
  record?: string;
};

/** The most names and patterns a tail follows (the server's `MOST_ASKED`). */
export const MOST_ASKED = 32;
export const LAST_CHOICES = [10, 50, 200] as const;
const NAME = /^[A-Za-z0-9._-]{1,249}$/;
const PATTERN = /^[A-Za-z0-9._\-*?]{1,249}$/;

function names(raw: unknown, re: RegExp): string[] | undefined {
  const all = Array.isArray(raw) ? raw : raw === undefined || raw === null ? [] : [raw];
  const out: string[] = [];
  for (const v of all) {
    const t = typeof v === "number" ? String(v) : typeof v === "string" ? v.trim() : "";
    if (re.test(t) && !out.includes(t) && out.length < MOST_ASKED) out.push(t);
  }
  return out.length ? out : undefined;
}

function partitions(raw: unknown): number[] | undefined {
  const all = Array.isArray(raw) ? raw : raw === undefined ? [] : [raw];
  const out = [...new Set(all.map((v) => (typeof v === "number" ? v : typeof v === "string" && /^\d{1,6}$/.test(v) ? Number(v) : NaN)).filter((n) => Number.isInteger(n) && n >= 0))].sort((a, b) => a - b);
  return out.length ? out : undefined;
}

function text(raw: unknown): string | undefined {
  return shortText(typeof raw === "number" ? String(raw) : typeof raw === "string" ? raw.trim() : undefined, 256);
}

export function tailSearch(s: Record<string, unknown>): TailSearch {
  const from = oneOf(["last"] as const, s.from);
  const last = typeof s.last === "number" ? s.last : typeof s.last === "string" && /^\d{1,3}$/.test(s.last) ? Number(s.last) : undefined;
  const header = text(s.header);
  const field = text(s.field);
  const record = typeof s.record === "string" && /^[A-Za-z0-9._-]{1,249}:\d{1,6}:\d{1,19}$/.test(s.record) ? s.record : undefined;
  return {
    topic: names(s.topic, NAME),
    pattern: names(s.pattern, PATTERN),
    from,
    last: from && last !== undefined && last >= 1 ? Math.min(last, 200) : undefined,
    partition: partitions(s.partition),
    key: text(s.key),
    value: text(s.value),
    header,
    header_value: header ? text(s.header_value) : undefined,
    field,
    equals: field ? text(s.equals) : undefined,
    isolation: oneOf(["read_committed"] as const, s.isolation),
    record,
  };
}

/** Whether the address follows anything (no topic and no pattern: nothing to tail, nothing is opened). */
export function following(s: TailSearch): boolean {
  return (s.topic?.length ?? 0) + (s.pattern?.length ?? 0) > 0;
}

/** The ask as the server reads it, without the pace and the Sheet: also the identity of a tail (a change of it starts anew). */
export function askQuery(s: TailSearch): string {
  const p = new URLSearchParams();
  for (const t of s.topic ?? []) p.append("topic", t);
  for (const t of s.pattern ?? []) p.append("pattern", t);
  if (s.from === "last") {
    p.set("from", "last");
    p.set("last", String(s.last ?? 10));
  }
  for (const n of s.partition ?? []) p.append("partition", String(n));
  for (const k of ["key", "value", "header", "header_value", "field", "equals"] as const) {
    const v = s[k];
    if (v) p.set(k, v);
  }
  if (s.isolation) p.set("isolation", s.isolation);
  return p.toString();
}

/** The filters that are on, for chips: a label, and the patch that turns it off. */
export function filterChips(s: TailSearch): { id: string; label: string; off: Partial<TailSearch> }[] {
  const out: { id: string; label: string; off: Partial<TailSearch> }[] = [];
  if (s.key) out.push({ id: "key", label: `key contains ${s.key}`, off: { key: undefined } });
  if (s.value) out.push({ id: "value", label: `value contains ${s.value}`, off: { value: undefined } });
  if (s.header) out.push({ id: "header", label: s.header_value ? `header ${s.header} contains ${s.header_value}` : `header ${s.header}`, off: { header: undefined, header_value: undefined } });
  if (s.field) out.push({ id: "field", label: s.equals !== undefined ? `${s.field} = ${s.equals}` : `has ${s.field}`, off: { field: undefined, equals: undefined } });
  for (const p of s.partition ?? []) out.push({ id: `p${p}`, label: `partition ${p}`, off: { partition: (s.partition ?? []).filter((x) => x !== p) } });
  if (s.isolation) out.push({ id: "isolation", label: "read_committed", off: { isolation: undefined } });
  return out.map((c) => (c.off.partition && c.off.partition.length === 0 ? { ...c, off: { partition: undefined } } : c));
}

/** `topic:partition:offset` of the record Sheet. */
export function recordOf(raw: string | undefined): { topic: string; partition: number; offset: number } | null {
  if (!raw) return null;
  const m = /^(.+):(\d+):(\d+)$/.exec(raw);
  return m ? { topic: m[1]!, partition: Number(m[2]), offset: Number(m[3]) } : null;
}

/** A path the server reads (`customer.id`, `items[0].sku`); the server says the same when it is not one. */
export function validPath(p: string): boolean {
  const t = p.trim().replace(/^\$\.?/, "");
  return t === "" || /^[A-Za-z0-9_\-$]+(\[\d+\])*(\.[A-Za-z0-9_\-$]+(\[\d+\])*)*$/.test(t);
}
