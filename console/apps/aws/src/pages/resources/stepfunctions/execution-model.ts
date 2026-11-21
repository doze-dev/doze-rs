// What an execution's page says in words: how long a step or the run took, the sentence under the title, what the graph shows, and
// what a failed or waiting step means (its Retry rules, the worker an activity waits for). Pure, so it is tested without a browser.
import type { AslState, Definition } from "./graph";
import { statesOf } from "./graph";
import type { Step } from "./history";
import { taskResource } from "./model";

/** `0.18 s`, `1.2 s`, `16 min 36 s`, `1 h 2 min`. */
export function durationWords(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0) return "–";
  if (ms < 1000) return `${(ms / 1000).toFixed(2).replace(/0$/, "")} s`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1).replace(/\.0$/, "")} s`;
  const s = Math.round(ms / 1000);
  if (s < 3600) return `${Math.floor(s / 60)} min ${s % 60} s`;
  return `${Math.floor(s / 3600)} h ${Math.floor((s % 3600) / 60)} min`;
}

/** `07:41:08.204`: an event's time of day in UTC, to the millisecond. */
export function clockMs(ms: number): string {
  return new Date(ms).toISOString().slice(11, 23);
}

const ONES = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
const ORDINAL = ["", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
const upper = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
const count = (n: number) => ONES[n] ?? String(n);

/** What the graph's lede says: `Three steps succeeded and the fourth failed. Choose a step to see its input, output and error.` */
export function graphWords(steps: readonly Pick<Step, "name" | "status" | "types">[], status: string): string {
  const done = steps.filter((s) => s.status === "succeeded").length;
  const bad = steps.find((s) => s.status === "failed");
  const open = steps.find((s) => s.status === "running");
  const ran = `${upper(count(done))} ${done === 1 ? "step" : "steps"} succeeded`;
  if (open && status === "RUNNING") {
    const wait = open.types.includes("ActivityScheduled") && !open.types.includes("ActivityStarted") ? ": no worker has taken the task yet" : "";
    return done === 0 ? `${open.name} is in progress${wait}.` : `${done === 1 ? "The first step" : `The first ${count(done)} steps`} succeeded. ${open.name} is in progress${wait}.`;
  }
  if (bad) {
    const n = steps.indexOf(bad) + 1;
    return `${ran} and the ${ORDINAL[n] ?? `${n}th`} ${bad.status === "failed" && status === "TIMED_OUT" ? "timed out" : "failed"}. Choose a step to see its input, output and error.`;
  }
  if (steps.length === 0) return "This execution has not entered a state yet.";
  return `${ran}. Choose a step to see its input, output and result.`;
}

/** The sentence under an execution's name. `version` and `alias` come from the ARN it was started with. */
export function executionWords(e: { status: string; startMs: number | null; stopMs: number | null }, machine: string, how: { version: string | null; alias: string | null }, steps: { current: string | null; last: string | null; activity: boolean }, when: (ms: number | null) => string): string {
  const via = how.alias ? `${how.alias}, version ${how.version ?? "?"}` : how.version ? `version ${how.version}` : "current revision";
  const started = `Started ${when(e.startMs)} on ${machine}, ${via}.`;
  const took = e.startMs !== null && e.stopMs !== null ? `It ran ${durationWords(e.stopMs - e.startMs)}` : "It ran";
  switch (e.status) {
    case "RUNNING":
      return `${started} ${steps.current ? (steps.activity ? `It is waiting for a worker on ${steps.current}.` : `It is running ${steps.current}.`) : "It is running."}`;
    case "SUCCEEDED":
      return `${started} ${took} and succeeded.`;
    case "FAILED":
      return `${started} ${took} and stopped at ${steps.last ?? "its first step"}.`;
    case "TIMED_OUT":
      return `${started} ${took} and timed out${steps.last ? ` at ${steps.last}` : ""}.`;
    case "ABORTED":
      return `${started} ${took} and was stopped${steps.last ? ` at ${steps.last}` : ""}.`;
    default:
      return `${started} Its status is ${e.status}.`;
  }
}

/** The Retry rule of a state that would catch `error`, or `null`. `States.ALL` catches everything. */
export function retryFor(state: AslState | undefined, error: string | undefined): NonNullable<AslState["Retry"]>[number] | null {
  if (!state || !error) return null;
  return (state.Retry ?? []).find((r) => (r.ErrorEquals ?? []).some((e) => e === error || e === "States.ALL" || (e === "States.TaskFailed" && !error.startsWith("States.")))) ?? null;
}

/** What the teaching line under a step says, as the boards say it. */
export function stepNote(def: Definition | undefined, step: Pick<Step, "name" | "status" | "error" | "types">, opts: { status: string; redrivable: boolean; pollers: string[]; }): { text: string; names: string[] } | null {
  const state = statesOf(def)[step.name];
  const names = [step.name];
  if (step.status === "running" && state && taskResource(state.Resource)?.activity) {
    const activity = taskResource(state.Resource)?.activity ?? "";
    const beat = typeof state.HeartbeatSeconds === "number" ? ` With HeartbeatSeconds ${state.HeartbeatSeconds} the worker must call SendTaskHeartbeat within that time.` : "";
    const poll = opts.pollers[0] ? ` ${opts.pollers[0]} polls ${activity};` : "";
    const timeout = typeof def?.TimeoutSeconds === "number" ? `${poll ? "" : " "}the machine's own timeout of ${def.TimeoutSeconds.toLocaleString("en-US")} seconds still applies.` : "";
    return { text: `${step.name} is an activity task. It stays in progress until a worker calls GetActivityTask for ${activity}, then answers with SendTaskSuccess or SendTaskFailure.${beat}${poll}${poll || timeout ? ` ${timeout}`.trimEnd() : ""}`.trim(), names: [...names, activity] };
  }
  if (step.status === "failed" && state) {
    const rule = retryFor(state, step.error);
    const rules = (state.Retry ?? []).flatMap((r) => r.ErrorEquals ?? []);
    const why = rule
      ? `The Retry rule on ${step.name} matches ${step.error}, so the step was retried up to ${rule.MaxAttempts ?? 3} times before it failed.`
      : rules.length > 0
        ? `The Retry rule on ${step.name} matches ${rules.join(", ")} only, so this error was not retried.`
        : `${step.name} has no Retry rule, so this error was not retried.`;
    const redo = opts.redrivable ? ` Redrive continues from ${step.name} with the same input, so it would fail the same way until what it calls or the input changes. A redrive is possible for 14 days after the execution ended.` : "";
    return { text: `${why}${redo}`, names };
  }
  return null;
}
