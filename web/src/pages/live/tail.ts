// The Live tail (live-design.md, CON-809/810): a stream the person switched on, read over fetch (the proof header), at the
// pace they chose, with rows merged by id (a folded repeat changes in place; a new row is prepended), a bounded tape (200)
// and a bounded hold (500), and a connection that says what it is and comes back by itself with no row twice.
import { useSyncExternalStore } from "react";
import { basePath } from "../../api/client";
import type { TailBatch } from "@/api/types/TailBatch";
import type { TailReady } from "@/api/types/TailReady";
import type { WireRow } from "@/api/types/WireRow";
import { SseParser } from "../../lib/sse";

// ---- the speed: one preference, remembered per browser ----

export const SPEEDS = [
  { value: "realtime", label: "Realtime", secs: 1, tip: "A batch of new rows at most once a second" },
  { value: "relaxed", label: "Relaxed", secs: 3, tip: "A batch of new rows at most every 3 seconds" },
  { value: "slow", label: "Slow", secs: 10, tip: "A batch of new rows at most every 10 seconds" },
] as const;
export type Speed = (typeof SPEEDS)[number]["value"];
export const SPEED_KEY = "doze:tail-speed";

export function parseSpeed(raw: string | null | undefined): Speed {
  return SPEEDS.find((s) => s.value === raw)?.value ?? "relaxed";
}
export function speedSecs(s: Speed): number {
  return SPEEDS.find((x) => x.value === s)?.secs ?? 3;
}

const speedListeners = new Set<() => void>();
let speed: Speed = (() => {
  try {
    return parseSpeed(globalThis.localStorage?.getItem(SPEED_KEY));
  } catch {
    return "relaxed";
  }
})();

export function setSpeed(s: Speed) {
  speed = s;
  try {
    globalThis.localStorage?.setItem(SPEED_KEY, s);
  } catch {
    // private mode: the choice lasts as long as the page
  }
  for (const l of speedListeners) l();
}

export function useSpeed(): [Speed, (s: Speed) => void] {
  const v = useSyncExternalStore(
    (l) => {
      speedListeners.add(l);
      return () => speedListeners.delete(l);
    },
    () => speed,
    () => "relaxed" as Speed,
  );
  return [v, setSpeed];
}

// ---- the tape: pure ----

/** The most rows the page keeps, and the most it holds aside while the person reads. */
export const TAPE_MOST = 200;
export const HELD_MOST = 500;

export type TapeRow = WireRow & { fresh?: true };

/** `incoming` (newest first) put into `rows`: a row already there is replaced where it is (a folded repeat that grew, the same
 * React key, so only its own cells change); a new one is prepended, marked fresh. The tape is cut at `most`. */
export function merge(rows: TapeRow[], incoming: WireRow[], most = TAPE_MOST): { rows: TapeRow[]; added: number } {
  if (incoming.length === 0) return { rows, added: 0 };
  const byId = new Map(incoming.map((r) => [r.id, r]));
  let touched = false;
  const kept = rows.map((r) => {
    const n = byId.get(r.id);
    if (!n) return r;
    byId.delete(r.id);
    touched = true;
    return r.fresh ? { ...n, fresh: true as const } : n;
  });
  const fresh: TapeRow[] = incoming.filter((r) => byId.has(r.id)).map((r) => ({ ...r, fresh: true as const }));
  if (fresh.length === 0 && !touched) return { rows, added: 0 };
  return { rows: [...fresh, ...kept].slice(0, most), added: fresh.length };
}

/** A batch put aside while the tape is held: newest first, one entry per id (the newest state of it), at most `most`. */
export function hold(held: WireRow[], incoming: WireRow[], most = HELD_MOST): WireRow[] {
  const ids = new Set(incoming.map((r) => r.id));
  return [...incoming, ...held.filter((r) => !ids.has(r.id))].slice(0, most);
}

/** How many held rows the tape does not have yet (what the pill counts). */
export function newCount(held: WireRow[], rows: TapeRow[]): number {
  const have = new Set(rows.map((r) => r.id));
  return held.filter((r) => !have.has(r.id)).length;
}

// ---- the connection ----

export type Conn = { state: "connecting" | "live" | "reconnecting" | "disconnected"; retryAt?: number };

/** The waits between attempts after a drop, in seconds: the first is "Reconnecting…", the rest "Disconnected · Retry in". */
export const BACKOFF = [1, 2, 4, 8, 15] as const;
/** A stream that has said nothing (not even its keep-alive, every 15 s) for this long is dead: a machine waking from sleep
 * leaves a socket open and silent. */
const SILENT_MS = 40_000;

export type TailHandlers = {
  onReady: (r: TailReady) => void;
  onBatch: (b: TailBatch) => void;
  onConn: (c: Conn) => void;
};

/** One tail: opens the stream, follows it, and after a drop opens it again from the last cursor it had. */
export class TailStream {
  private cursor: string;
  private stopped = false;
  private abort: AbortController | null = null;
  private wake: (() => void) | null = null;
  private failures = 0;

  constructor(
    private readonly query: string,
    from: string,
    private readonly pace: number,
    private readonly h: TailHandlers,
  ) {
    this.cursor = from;
  }

  start() {
    void this.run();
  }

  stop() {
    this.stopped = true;
    this.abort?.abort();
    this.wake?.();
  }

  /** Try again now (the Retry of the red indicator). */
  retry() {
    this.wake?.();
  }

  private url(): string {
    const p = new URLSearchParams(this.query);
    p.set("pace", String(this.pace));
    return `${basePath}/api/v1/tail/live?${p.toString()}`;
  }

  private async run() {
    while (!this.stopped) {
      // While an attempt is out the indicator keeps what it said (no amber-red-amber flicker on every try).
      this.h.onConn(this.failures === 0 ? { state: "connecting" } : this.failures === 1 ? { state: "reconnecting" } : { state: "disconnected" });
      try {
        await this.once();
      } catch {
        // dropped, refused or unreachable: the wait below
      }
      if (this.stopped) return;
      this.failures += 1;
      const wait = BACKOFF[Math.min(this.failures - 1, BACKOFF.length - 1)]! * 1000;
      this.h.onConn(this.failures === 1 ? { state: "reconnecting" } : { state: "disconnected", retryAt: Date.now() + wait });
      await new Promise<void>((resolve) => {
        const t = setTimeout(resolve, wait);
        this.wake = () => {
          clearTimeout(t);
          resolve();
        };
      });
      this.wake = null;
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
      const res = await fetch(this.url(), {
        headers: { "X-Console": "1", Accept: "text/event-stream", "Last-Event-ID": this.cursor },
        credentials: "same-origin",
        cache: "no-store",
        signal: abort.signal,
      });
      if (!res.ok || !res.body) throw new Error(`tail: ${res.status}`);
      quiet();
      const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
      const parser = new SseParser();
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        quiet();
        for (const e of parser.push(value)) {
          if (this.stopped) return;
          if (e.event === "ready") {
            const r = JSON.parse(e.data) as TailReady;
            this.cursor = r.cursor;
            this.failures = 0;
            this.h.onReady(r);
            this.h.onConn({ state: "live" });
          } else if (e.event === "rows") {
            const b = JSON.parse(e.data) as TailBatch;
            this.cursor = e.id ?? b.cursor;
            this.h.onBatch(b);
          }
        }
      }
    } finally {
      clearTimeout(silent);
      if (this.abort === abort) this.abort = null;
    }
  }
}
