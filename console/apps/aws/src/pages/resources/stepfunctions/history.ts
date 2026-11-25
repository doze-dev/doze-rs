// What an execution's history says of each state: whether it ran, is running, or failed; what it took and gave; and which
// edges the run took. Read from GetExecutionHistory's events (oldest first) as the model has them. Pure, so it is tested
// without a browser.

export type HistoryEvent = {
  id: number;
  previousEventId?: number;
  timestamp: number | string;
  type: string;
  stateEnteredEventDetails?: { name: string; input?: string };
  stateExitedEventDetails?: { name: string; output?: string };
  [details: string]: unknown;
};

export type StepStatus = "succeeded" | "failed" | "running" | "cancelled";
export type Step = { name: string; status: StepStatus; input?: string; output?: string; error?: string; cause?: string; entered: number; events: number[]; /** When the state was entered and when it ended (its last event), ms since 1970. */ startMs?: number; endMs?: number; /** The types of its events. */ types: string[] };

/** An event's time as ms since 1970 (the service answers seconds with a fraction, or ms, or ISO text). */
export function tsMs(v: number | string): number {
  if (typeof v === "number") return v > 1e12 ? v : v * 1000;
  const n = Number(v);
  return Number.isFinite(n) ? (n > 1e12 ? n : n * 1000) : Date.parse(v);
}
export type Run = { steps: Map<string, Step>; taken: Set<string> };

/** The error and cause an event carries in its details, if any (`taskFailedEventDetails`, `executionFailedEventDetails`…). */
export function failureOf(e: HistoryEvent): { error?: string; cause?: string } | null {
  for (const [k, v] of Object.entries(e)) {
    if (!k.endsWith("EventDetails") || typeof v !== "object" || v === null) continue;
    const d = v as { error?: unknown; cause?: unknown };
    if (typeof d.error === "string" || typeof d.cause === "string") return { error: typeof d.error === "string" ? d.error : undefined, cause: typeof d.cause === "string" ? d.cause : undefined };
  }
  return null;
}

/** The execution's status word as a step's, for the states still open when it ended. */
function closing(status: string): StepStatus | null {
  if (status === "FAILED" || status === "TIMED_OUT") return "failed";
  if (status === "ABORTED") return "cancelled";
  return null;
}

/** Each state's step, and the edges taken (`from>to`, by name), from `events` oldest first; `status` is the execution's. */
export function readRun(events: HistoryEvent[], status: string): Run {
  const steps = new Map<string, Step>();
  const exitedBy = new Map<number, string>();
  const taken = new Set<string>();
  const open: string[] = [];
  let firstEntered = true;
  for (const e of events) {
    const entered = e.stateEnteredEventDetails;
    const exited = e.stateExitedEventDetails;
    if (entered) {
      const from = e.previousEventId === undefined ? undefined : exitedBy.get(e.previousEventId);
      if (from) taken.add(`${from}>${entered.name}`);
      else if (firstEntered) taken.add(`__start>${entered.name}`);
      firstEntered = false;
      steps.set(entered.name, { name: entered.name, status: "running", input: entered.input, entered: e.id, events: [e.id], startMs: tsMs(e.timestamp), types: [e.type] });
      open.push(entered.name);
    } else if (exited) {
      exitedBy.set(e.id, exited.name);
      const s = steps.get(exited.name);
      if (s) {
        s.status = "succeeded";
        s.output = exited.output;
        s.events.push(e.id);
        s.types.push(e.type);
        s.endMs = tsMs(e.timestamp);
      }
      const at = open.lastIndexOf(exited.name);
      if (at >= 0) open.splice(at, 1);
    } else {
      // An event inside the newest open state (a task scheduled, started, failed).
      const name = open[open.length - 1];
      const s = name ? steps.get(name) : undefined;
      if (s) {
        s.events.push(e.id);
        s.types.push(e.type);
        if (!e.type.startsWith("Execution")) s.endMs = tsMs(e.timestamp);
        const f = failureOf(e);
        if (f && /Failed|TimedOut|Aborted/.test(e.type) && !e.type.startsWith("Execution")) {
          s.error = f.error;
          s.cause = f.cause;
        }
      }
      if (e.type === "ExecutionSucceeded") {
        const last = [...exitedBy.values()].pop();
        if (last) taken.add(`${last}>__end`);
      }
    }
  }
  const end = closing(status);
  if (end) {
    const lastFailure = [...events].reverse().map(failureOf).find(Boolean);
    for (const name of open) {
      const s = steps.get(name);
      if (!s || s.status !== "running") continue;
      s.status = end;
      if (end === "failed" && !s.error && lastFailure) {
        s.error = lastFailure.error;
        s.cause = lastFailure.cause;
      }
    }
  }
  return { steps, taken };
}

/** How a status is said and coloured. */
export const STATUS: Record<string, { word: string; tone: "ok" | "bad" | "info" | "warn" | "idle" }> = {
  RUNNING: { word: "Running", tone: "info" },
  SUCCEEDED: { word: "Succeeded", tone: "ok" },
  FAILED: { word: "Failed", tone: "bad" },
  TIMED_OUT: { word: "Timed out", tone: "bad" },
  ABORTED: { word: "Aborted", tone: "idle" },
  PENDING_REDRIVE: { word: "Pending redrive", tone: "info" },
  succeeded: { word: "Succeeded", tone: "ok" },
  failed: { word: "Failed", tone: "bad" },
  running: { word: "In progress", tone: "info" },
  cancelled: { word: "Cancelled", tone: "warn" },
};

/** The state each event happened in, by event id: the newest state still open when it came (`TaskFailed` is `NotifyCustomer`'s). */
export function statesOfEvents(events: readonly HistoryEvent[]): Map<number, string> {
  const of = new Map<number, string>();
  const open: string[] = [];
  for (const e of events) {
    if (e.stateEnteredEventDetails) {
      open.push(e.stateEnteredEventDetails.name);
      of.set(e.id, e.stateEnteredEventDetails.name);
    } else if (e.stateExitedEventDetails) {
      of.set(e.id, e.stateExitedEventDetails.name);
      const at = open.lastIndexOf(e.stateExitedEventDetails.name);
      if (at >= 0) open.splice(at, 1);
    } else {
      const state = open[open.length - 1];
      if (state && !e.type.startsWith("Execution")) of.set(e.id, state);
    }
  }
  return of;
}

const FAILING = /Failed|TimedOut|Aborted/;

/** The event to open first on the event view: the first one that failed inside a state (`TaskFailed`), else the execution's own
 * failure (`ExecutionFailed`), else none. */
export function firstFailure(events: readonly HistoryEvent[]): HistoryEvent | null {
  return events.find((e) => FAILING.test(e.type) && !e.type.startsWith("Execution")) ?? events.find((e) => FAILING.test(e.type)) ?? null;
}
