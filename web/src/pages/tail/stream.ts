// The Live tail page's stream (CON-809, CON-1583): opened when a person chose what to follow, read over fetch (the proof
// header), at the pace they chose; rows merged by a stable key (topic, partition, offset), the newest 200 on screen and 500
// held while they read; a dropped connection, a pause or a hidden tab goes on from the cursor of the last batch, so nothing
// comes twice and nothing written meanwhile is missed (within the server's bounds: a partition far behind is skipped ahead,
// and the batch says how many).
import { basePath } from "../../api/client";
import type { RecordView } from "@/api/types/RecordView";
import type { Shown } from "@/api/types/Shown";
import type { TailRecord } from "@/api/types/TailRecord";
import type { TopicsTailBatch } from "@/api/types/TopicsTailBatch";
import type { TopicsTailReady } from "@/api/types/TopicsTailReady";
import { untilStaying } from "../../lib/leaving";
import { shellWord } from "../../lib/shell";
import { SseParser } from "../../lib/sse";

// ---- rows: pure ----

export const WINDOW = 200;
export const MOST_HELD = 500;

export type Row = TailRecord & { fresh?: true };

export const rowKey = (r: { topic: string; partition: number; offset: number }) => `${r.topic}:${r.partition}:${r.offset}`;

/** `incoming` (newest first) put on top of `rows`: a row already there is never rewritten, the new ones are marked fresh,
 * and the window is cut at `most`. */
export function prepend(rows: Row[], incoming: TailRecord[], most = WINDOW): Row[] {
  if (incoming.length === 0) return rows;
  const have = new Set(rows.map(rowKey));
  const fresh: Row[] = [];
  for (const r of incoming) {
    const k = rowKey(r);
    if (have.has(k)) continue;
    have.add(k);
    fresh.push({ ...r, fresh: true });
  }
  if (fresh.length === 0) return rows;
  const kept = rows.map((r) => {
    if (!r.fresh) return r;
    const c = { ...r };
    delete c.fresh;
    return c;
  });
  return [...fresh, ...kept].slice(0, most);
}

/** Rows put aside while the person reads: newest first, none twice, at most `most`. */
export function hold(held: TailRecord[], incoming: TailRecord[], most = MOST_HELD): TailRecord[] {
  const keys = new Set(held.map(rowKey));
  return [...incoming.filter((r) => !keys.has(rowKey(r))), ...held].slice(0, most);
}

// ---- the rate: records a second, per topic, over the last few seconds ----

export const RATE_WINDOW_MS = 10_000;

export type Sample = { at: number; counts: { topic: string; scanned: number; matched: number }[] };

/** Records a second of each topic over the samples of the last {@link RATE_WINDOW_MS} before `now` (scanned: what was
 * written; matched: what the filters kept). The window is never shorter than the pace, so a slow tail does not read as 0. */
export function rates(samples: Sample[], now: number, paceMs: number): Map<string, { scanned: number; matched: number }> {
  const span = Math.max(RATE_WINDOW_MS, paceMs * 2);
  const out = new Map<string, { scanned: number; matched: number }>();
  for (const s of samples) {
    if (now - s.at > span) continue;
    for (const c of s.counts) {
      const r = out.get(c.topic) ?? { scanned: 0, matched: 0 };
      r.scanned += c.scanned;
      r.matched += c.matched;
      out.set(c.topic, r);
    }
  }
  for (const [t, r] of out) out.set(t, { scanned: r.scanned / (span / 1000), matched: r.matched / (span / 1000) });
  return out;
}

/** `0`, `0.4`, `12`, `1.2k` records a second. */
export function rateText(perSec: number): string {
  if (perSec <= 0) return "0";
  if (perSec < 10) return (Math.round(perSec * 10) / 10).toString();
  if (perSec < 1000) return Math.round(perSec).toString();
  return `${(Math.round(perSec / 100) / 10).toString()}k`;
}

// ---- a topic's colour: stable for its name, one of eight theme hues ----

export const HUES = 8;
export function hueOf(topic: string): number {
  let h = 2166136261;
  for (let i = 0; i < topic.length; i++) {
    h ^= topic.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % HUES) + 1;
}

/** The colours given to the topics on screen: by name, and when two of the followed topics fall on one hue, the later one
 * (by name) takes the next free hue, so every topic of a tail of up to eight has its own. */
export function huesOf(topics: string[]): Map<string, number> {
  const out = new Map<string, number>();
  const used = new Set<number>();
  for (const t of [...new Set(topics)].sort()) {
    let h = hueOf(t);
    for (let i = 0; i < HUES && used.has(h) && used.size < HUES; i++) h = (h % HUES) + 1;
    used.add(h);
    out.set(t, h);
  }
  return out;
}

/** One line of a key or a value for a cell: the schema's preview when it has one, the text with its line breaks as spaces. */
export function lineOf(s: Shown): string {
  if (s.schema) return s.schema.preview;
  if (s.kind === "null") return "null";
  if (s.kind === "empty") return "empty";
  if (s.kind === "binary") return s.preview;
  return s.text.replace(/\s+/g, " ").trim() + (s.clipped ? " …" : "");
}

// ---- copy a record as a command that produces it again ----

/** Why a record cannot be written again as text from a shell (binary, cut short, or a character the page wrote out). */
function notText(s: Shown, what: string): string | null {
  if (s.kind === "binary") return `Its ${what} is binary`;
  if (s.clipped) return `Its ${what} is longer than the console reads`;
  if (s.schema) return `Its ${what} is in the schema registry's wire format`;
  if (/⟨U\+[0-9A-F]{4,}⟩/.test(s.text)) return `Its ${what} holds control characters`;
  return null;
}

export type Commands = { kcat: string; producer: string } | { why: string };

/** `kcat -P` and `kafka-console-producer` commands that write `r` again to its topic and partition: its key, its value and
 * its headers, every word quoted for a shell. A record whose bytes are not plain text cannot be, and says why. */
export function commands(r: RecordView, address: string): Commands {
  const why = notText(r.value, "value") ?? (r.key.kind === "null" ? null : notText(r.key, "key")) ?? r.headers.map((h) => (h.value.kind === "null" ? null : notText(h.value, `header ${h.key}`))).find((w) => w) ?? null;
  if (why) return { why: `${why}: copy it from the record instead.` };
  if (r.value.kind === "null") return { why: "It is a deletion (a null value): kcat -P -Z writes one, the console producer cannot." };
  const value = r.value.kind === "empty" ? "" : r.value.text;
  const key = r.key.kind === "null" ? null : r.key.kind === "empty" ? "" : r.key.text;
  const tab = /\t|\n/;
  const headers = r.headers.map((h) => [h.key, h.value.kind === "null" || h.value.kind === "empty" ? "" : h.value.text] as const);
  const kcat = [
    `printf '%s' ${shellWord(value)} |`,
    `kcat -P -b ${shellWord(address)} -t ${shellWord(r.topic)} -p ${r.partition}`,
    key !== null ? `-k ${shellWord(key)}` : null,
    ...headers.map(([k, v]) => `-H ${shellWord(`${k}=${v}`)}`),
  ]
    .filter(Boolean)
    .join(" ");
  if (tab.test(value) || (key !== null && tab.test(key)) || headers.some(([k, v]) => tab.test(k) || tab.test(v) || /[,:]/.test(k) || /,/.test(v))) {
    return { kcat, producer: "# kafka-console-producer reads one record a line, split at tabs: this record holds a tab or a line break." };
  }
  const line = [headers.length ? headers.map(([k, v]) => `${k}:${v}`).join(",") : null, key, value].filter((x) => x !== null).join("\t");
  const props = [key !== null ? "--property parse.key=true" : null, headers.length ? "--property parse.headers=true" : null].filter(Boolean).join(" ");
  const producer = `printf '%s\\n' ${shellWord(line)} |\nkafka-console-producer --bootstrap-server ${shellWord(address)} --topic ${shellWord(r.topic)}${props ? ` ${props}` : ""}`;
  return { kcat, producer };
}

// ---- the connection ----

export type Conn =
  | { state: "connecting" | "live" | "reconnecting" | "paused" | "hidden" }
  | { state: "disconnected"; retryAt: number; why: string };

export const BACKOFF = [1, 2, 4, 8, 15] as const;
/** A stream that has said nothing (not even its keep-alive, every 15 s) for this long is dead. */
const SILENT_MS = 40_000;

export type Handlers = {
  onReady: (r: TopicsTailReady) => void;
  onBatch: (b: TopicsTailBatch) => void;
  onConn: (c: Conn) => void;
};

/** One tail of topics: opens the stream, follows it, and after a drop, a pause or a hidden tab opens it again from the
 * cursor of the last event it had (`Last-Event-ID`). */
export class TopicsTail {
  private cursor: string | null = null;
  private stopped = false;
  private held: "paused" | "hidden" | null = null;
  private abort: AbortController | null = null;
  private wake: (() => void) | null = null;
  private failures = 0;
  private running = false;

  constructor(
    private readonly query: string,
    private readonly pace: number,
    private readonly h: Handlers,
    /** Where to go on from (the cursor of a tail of the same ask at another pace). */
    from: string | null = null,
  ) {
    this.cursor = from;
  }

  start() {
    if (!this.running) void this.run();
  }

  stop() {
    this.stopped = true;
    this.abort?.abort();
    this.wake?.();
  }

  /** Stops reading (the Pause button, a hidden tab) and keeps the cursor; {@link go} reads on from it. */
  hold(why: "paused" | "hidden") {
    if (this.held === "paused" && why === "hidden") return;
    this.held = why;
    this.abort?.abort();
    this.wake?.();
    this.h.onConn({ state: why });
  }

  /** Reads on from the cursor: after a pause (`paused`) or when the tab is seen again (`hidden`, which never undoes a pause). */
  go(why: "paused" | "hidden") {
    if (this.held === null || (why === "hidden" && this.held === "paused")) return;
    this.held = null;
    this.failures = 0;
    this.start();
  }

  retry() {
    this.wake?.();
  }

  /** Where the tail stands (the cursor a reconnect sends). */
  where(): string | null {
    return this.cursor;
  }

  private url(): string {
    const p = new URLSearchParams(this.query);
    p.set("pace", String(this.pace));
    return `${basePath}/api/v1/tail/topics?${p.toString()}`;
  }

  private async run() {
    this.running = true;
    try {
      while (!this.stopped && this.held === null) {
        this.h.onConn(this.failures === 0 ? { state: this.cursor ? "reconnecting" : "connecting" } : { state: "reconnecting" });
        let why = "The stream ended.";
        try {
          await this.once();
        } catch (e) {
          why = (e as Error).message || why;
        }
        if (this.stopped || this.held !== null) return;
        this.failures += 1;
        const wait = BACKOFF[Math.min(this.failures - 1, BACKOFF.length - 1)]! * 1000;
        this.h.onConn(this.failures === 1 ? { state: "reconnecting" } : { state: "disconnected", retryAt: Date.now() + wait, why });
        await new Promise<void>((resolve) => {
          const t = setTimeout(resolve, wait);
          this.wake = () => {
            clearTimeout(t);
            resolve();
          };
        });
        this.wake = null;
      }
    } finally {
      this.running = false;
    }
  }

  private async once() {
    const abort = new AbortController();
    this.abort = abort;
    let silent: ReturnType<typeof setTimeout> | undefined;
    const quiet = () => {
      clearTimeout(silent);
      silent = setTimeout(() => abort.abort(), SILENT_MS);
    };
    try {
      await untilStaying(abort.signal);
      const headers: Record<string, string> = { "X-Console": "1", Accept: "text/event-stream" };
      if (this.cursor) headers["Last-Event-ID"] = this.cursor;
      const res = await fetch(this.url(), { headers, credentials: "same-origin", cache: "no-store", signal: abort.signal });
      if (!res.ok || !res.body) {
        let message = `The console answered ${res.status}.`;
        try {
          message = ((await res.json()) as { message?: string }).message ?? message;
        } catch {
          // not JSON
        }
        throw new Error(message);
      }
      quiet();
      const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
      const parser = new SseParser();
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        quiet();
        for (const e of parser.push(value)) {
          if (this.stopped || this.held !== null) return;
          if (e.event === "ready") {
            const r = JSON.parse(e.data) as TopicsTailReady;
            this.cursor = r.cursor || this.cursor;
            this.failures = 0;
            this.h.onReady(r);
            this.h.onConn({ state: "live" });
          } else if (e.event === "batch") {
            const b = JSON.parse(e.data) as TopicsTailBatch;
            this.cursor = e.id ?? b.cursor;
            this.h.onBatch(b);
          }
        }
      }
    } catch (e) {
      if ((e as Error).name === "AbortError" && (this.stopped || this.held !== null)) return;
      throw e;
    } finally {
      clearTimeout(silent);
      if (this.abort === abort) this.abort = null;
    }
  }
}
