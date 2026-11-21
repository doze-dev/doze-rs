// Repeats on the wire: calls in a row with the same caller, operation, resource and outcome are one row with a count (a client
// polling a queue is one line, not a screenful). A run opens to show each of its calls. Pure: the page draws what this returns.
import type { CallRecord } from "../../api/types/CallRecord";
import { callerOf } from "./search";

/** One line of the wire: a call, a run of repeats (newest first in `calls`), or one call of an open run. */
export type Line =
  | { kind: "call"; call: CallRecord }
  | { kind: "run"; call: CallRecord; calls: CallRecord[]; open: boolean }
  | { kind: "member"; call: CallRecord; of: number };

/** What makes two calls repeats of each other. */
export function sameAs(a: CallRecord, b: CallRecord): boolean {
  return callerOf(a) === callerOf(b) && a.service === b.service && a.action === b.action && a.resource === b.resource && a.outcome === b.outcome && a.would_be_denied === b.would_be_denied && (a.via === null) === (b.via === null);
}

/** The id of a run: its oldest call's number (it does not change as the run grows at the top). */
export const runId = (calls: CallRecord[]) => calls[calls.length - 1]!.seq;

/** `calls` (newest first) as lines; the runs whose ids are in `open` are followed by their other calls (the run's own line is
 * its newest). */
export function lines(calls: CallRecord[], open: ReadonlySet<number>): Line[] {
  const out: Line[] = [];
  let i = 0;
  while (i < calls.length) {
    const first = calls[i]!;
    let j = i + 1;
    while (j < calls.length && sameAs(first, calls[j]!)) j++;
    const run = calls.slice(i, j);
    if (run.length === 1) {
      out.push({ kind: "call", call: first });
    } else {
      const id = runId(run);
      const isOpen = open.has(id);
      out.push({ kind: "run", call: first, calls: run, open: isOpen });
      if (isOpen) for (const c of run.slice(1)) out.push({ kind: "member", call: c, of: id });
    }
    i = j;
  }
  return out;
}

/** The row id of a line (the table keys rows by it; `data-row` is the call's number). */
export function lineId(l: Line): string {
  return l.kind === "member" ? `${l.of}:${l.call.seq}` : String(l.call.seq);
}
