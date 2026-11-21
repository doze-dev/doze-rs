// The Live tape: the calls on screen, newest first, kept current by pages of `/api/v1/calls` and the `calls` events of
// `/api/v1/tail/live`. Pure: the page feeds it what the server sent and what the person pressed, and draws what it holds.
//
// - A call is on the tape once (the server numbers them in order: one at or under the newest seen is a repeat).
// - While paused the stream stays open and its calls are held aside, counted; Resume puts them on the tape.
// - The tape and the hold are bounded by the record's own capacity: what the server no longer holds, the page does not either.
//   The server lets this console's own calls go first once they pass a quarter of it (ring.rs): the tape does the same ({@link admit}).
// - `reset` (doze-aws restarted: the cursor was another run's) drops everything held from the old run.
import type { CallRecord } from "../../api/types/CallRecord";
import type { CallsPage } from "../../api/types/CallsPage";
import type { RingFill } from "../../api/types/RingFill";
import type { TailReady } from "../../api/types/TailReady";

/** The most calls kept before the server has said its capacity. */
export const DEFAULT_CAP = 1024;

export type Tape = {
  /** On screen, newest first. */
  calls: CallRecord[];
  /** Arrived while paused, newest first. */
  held: CallRecord[];
  paused: boolean;
  /** What to ask the stream to continue from; null before the first page. */
  cursor: string | null;
  ring: RingFill | null;
  /** Calls the server recorded and dropped before this page was given them (and held ones dropped here past the cap). */
  missed: number;
  /** A call numbered above this arrived by the stream since the last batch: its row is highlighted once. */
  freshAfter: number;
  /** The first page has arrived. */
  loaded: boolean;
};

export const EMPTY: Tape = { calls: [], held: [], paused: false, cursor: null, ring: null, missed: 0, freshAfter: Number.MAX_SAFE_INTEGER, loaded: false };

export type TapeAction = { type: "loaded"; page: CallsPage } | { type: "ready"; ready: TailReady } | { type: "calls"; page: CallsPage } | { type: "pause" } | { type: "resume" };

/** The newest call number the tape has seen, on screen or held; 0 for none. */
export function newestSeq(t: Tape): number {
  return Math.max(t.calls[0]?.seq ?? 0, t.held[0]?.seq ?? 0);
}

const cap = (t: Tape) => t.ring?.capacity ?? DEFAULT_CAP;

/** The console's own calls, which the record lets go first past a quarter of it. */
const CONSOLE = "console";
export const CONSOLE_SHARE = 4;

/** `fresh` (newest first) put on `calls` (newest first) one by one, oldest first, as the record takes them: past `most`, the
 * oldest of this console's own calls leaves while they are more than a quarter of `most`, else the oldest call. */
export function admit(calls: CallRecord[], fresh: CallRecord[], most: number): CallRecord[] {
  const out = [...calls];
  let own = out.filter((c) => c.caller === CONSOLE).length;
  for (let i = fresh.length - 1; i >= 0; i--) {
    const c = fresh[i]!;
    // As the record does: a full one first lets one go (judged before the new call is counted), then takes the new one.
    if (out.length >= most) {
      let at = out.length - 1;
      if (own > Math.floor(most / CONSOLE_SHARE)) while (at > 0 && out[at]!.caller !== CONSOLE) at--;
      const [gone] = out.splice(at, 1);
      if (gone?.caller === CONSOLE) own--;
    }
    out.unshift(c);
    if (c.caller === CONSOLE) own++;
  }
  return out;
}

/** `page.calls` (oldest first) as the tape keeps them (newest first), without those at or under `after`. */
function arrivals(page: CallsPage, after: number): CallRecord[] {
  return page.calls.filter((c) => c.seq > after).reverse();
}

export function reduce(t: Tape, a: TapeAction): Tape {
  switch (a.type) {
    case "loaded": {
      // A page asked for with no cursor: the newest the record holds. It replaces the tape (a reload after an error too).
      const calls = arrivals(a.page, 0).slice(0, a.page.ring.capacity);
      return { ...t, calls, held: [], cursor: a.page.cursor, ring: a.page.ring, missed: 0, freshAfter: calls[0]?.seq ?? 0, loaded: true };
    }
    case "ready":
      // The stream is open. A reset means the cursor was another run's: what is on screen never happened in this run.
      return a.ready.reset ? { ...t, calls: [], held: [], cursor: a.ready.cursor, missed: 0, freshAfter: 0 } : t;
    case "calls": {
      const base = a.page.reset ? { ...t, calls: [], held: [], missed: 0, freshAfter: 0 } : t;
      const seen = newestSeq(base);
      const fresh = arrivals(a.page, seen);
      const next = { ...base, cursor: a.page.cursor, ring: a.page.ring, missed: base.missed + a.page.missed };
      const most = a.page.ring.capacity;
      if (base.paused) {
        const held = [...fresh, ...base.held];
        return { ...next, held: held.slice(0, most), missed: next.missed + Math.max(0, held.length - most) };
      }
      if (fresh.length === 0) return next;
      return { ...next, calls: admit(base.calls, fresh, most), freshAfter: seen };
    }
    case "pause":
      return t.paused ? t : { ...t, paused: true };
    case "resume": {
      if (!t.paused) return t;
      if (t.held.length === 0) return { ...t, paused: false };
      return { ...t, paused: false, held: [], calls: admit(t.calls, t.held, cap(t)), freshAfter: t.calls[0]?.seq ?? 0 };
    }
  }
}

/** How long to wait before the stream is tried again, by how many tries in a row have failed: 1 s, 2 s, 5 s, then 10 s. */
export function retryDelayMs(failures: number): number {
  return [1_000, 2_000, 5_000][Math.max(0, failures - 1)] ?? 10_000;
}

/** The call numbered `seq`, on the tape or held. */
export function findCall(t: Tape, seq: number | undefined): CallRecord | undefined {
  if (seq === undefined) return undefined;
  return t.calls.find((c) => c.seq === seq) ?? t.held.find((c) => c.seq === seq);
}
