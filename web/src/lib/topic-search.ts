// The typed search params of the Topics pages (slice R1a): every filter is in the address, so Back, a reload and a shared
// link all show the same page. Parsed by hand like `search.ts` (no schema library in the main chunk): a value that is not one
// of the allowed ones is dropped, never shown.
import { shortText } from "./search";

/** A whole number from 0 (the router hands numbers over parsed, a hand-edited address hands text). */
export function whole(raw: unknown): number | undefined {
  const n = typeof raw === "number" ? raw : typeof raw === "string" && /^\d{1,15}$/.test(raw.trim()) ? Number(raw.trim()) : NaN;
  return Number.isSafeInteger(n) && n >= 0 ? n : undefined;
}

/** `dialog` and `name` are for R1b (Create topic, with a name to start from). */
export type TopicsSearch = { q?: string; internal?: true; dialog?: string; name?: string };
export function topicsSearch(s: Record<string, unknown>): TopicsSearch {
  return { dialog: shortText(s.dialog, 32), name: shortText(s.name, 249), q: shortText(typeof s.q === "number" ? String(s.q) : s.q, 249), internal: s.internal === true || s.internal === 1 || s.internal === "1" || s.internal === "true" ? true : undefined };
}

/** The overlays of a topic's pages (R1b mounts them): `?dialog=produce`. */
export type TopicSearch = { dialog?: string };
export function topicSearch(s: Record<string, unknown>): TopicSearch {
  return { dialog: shortText(s.dialog, 32) };
}

export type Isolation = "read_uncommitted" | "read_committed";

export type MessagesSearch = TopicSearch & {
  /** `0` or `0,2`: the partitions read; none is all. */
  partition?: string;
  offset?: number;
  /** As typed: `2026-11-13 14:30` (UTC) or milliseconds. */
  time?: string;
  key?: string;
  isolation?: "read_committed";
  limit?: number;
  /** The record Sheet: `partition:offset`. */
  record?: string;
  /** The Live tail. */
  mode?: "tail";
};

export function partitionsOf(raw: string | undefined): number[] {
  if (!raw) return [];
  const out = new Set<number>();
  for (const p of raw.split(",")) {
    const n = whole(p);
    if (n !== undefined && n <= 1_000_000) out.add(n);
  }
  return [...out].sort((a, b) => a - b);
}

export function messagesSearch(s: Record<string, unknown>): MessagesSearch {
  const parts = partitionsOf(typeof s.partition === "number" ? String(s.partition) : typeof s.partition === "string" ? s.partition : undefined);
  const record = typeof s.record === "string" && /^\d{1,9}:\d{1,18}$/.test(s.record) ? s.record : undefined;
  const limit = whole(s.limit);
  return {
    ...topicSearch(s),
    partition: parts.length ? parts.join(",") : undefined,
    offset: whole(s.offset),
    time: shortText(typeof s.time === "number" ? String(s.time) : s.time, 40),
    key: shortText(typeof s.key === "number" ? String(s.key) : s.key, 512),
    isolation: s.isolation === "read_committed" ? "read_committed" : undefined,
    limit: limit && limit > 0 && limit <= 200 ? limit : undefined,
    record,
    mode: s.mode === "tail" ? "tail" : undefined,
  };
}

/** The API's query for a Messages search (`partition=0&partition=2&isolation=read_committed`). */
export function messagesQuery(s: MessagesSearch): string {
  const q = new URLSearchParams();
  for (const p of partitionsOf(s.partition)) q.append("partition", String(p));
  if (s.offset !== undefined) q.set("offset", String(s.offset));
  if (s.time) q.set("time", s.time);
  if (s.key) q.set("key", s.key);
  if (s.isolation) q.set("isolation", s.isolation);
  if (s.limit) q.set("limit", String(s.limit));
  return q.toString();
}

/** `1:16050` as a place. */
export function recordAt(raw: string | undefined): { partition: number; offset: number } | null {
  if (!raw) return null;
  const [p, o] = raw.split(":");
  const partition = whole(p);
  const offset = whole(o);
  return partition === undefined || offset === undefined ? null : { partition, offset };
}

export type PartitionSearch = TopicSearch & { partition?: number };
export function partitionSearch(s: Record<string, unknown>): PartitionSearch {
  const p = whole(s.partition);
  return { ...topicSearch(s), partition: p && p > 0 ? p : undefined };
}
