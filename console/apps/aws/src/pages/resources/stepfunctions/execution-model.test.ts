import { describe, expect, test } from "bun:test";
import { causeHint, clockMs, durationWords, executionWords, graphWords, redriveWarning, retryFor, stepNote } from "./execution-model";

const when = (ms: number | null) => (ms === null ? "–" : "Nov 8, 2026 07:41:08");

describe("durations", () => {
  test("are said as the boards say them", () => {
    expect(durationWords(20)).toBe("0.02 s");
    expect(durationWords(180)).toBe("0.18 s");
    expect(durationWords(1200)).toBe("1.2 s");
    expect(durationWords(996_000)).toBe("16 min 36 s");
    expect(durationWords(3_720_000)).toBe("1 h 2 min");
    expect(durationWords(null)).toBe("–");
  });
  test("a clock keeps the milliseconds", () => {
    expect(clockMs(Date.UTC(2026, 10, 8, 7, 41, 8, 204))).toBe("07:41:08.204");
  });
});

describe("the graph's lede", () => {
  const ok = (name: string) => ({ name, status: "succeeded" as const, types: [] });
  test("names the step that failed by its place", () => {
    expect(graphWords([ok("A"), ok("B"), ok("C"), { name: "D", status: "failed", types: [] }], "FAILED")).toBe("Three steps succeeded and the fourth failed. Choose a step to see its input, output and error.");
  });
  test("says when no worker has taken an activity's task", () => {
    expect(graphWords([ok("A"), ok("B"), { name: "C", status: "running", types: ["TaskStateEntered", "ActivityScheduled"] }], "RUNNING")).toBe("The first two steps succeeded. C is in progress: no worker has taken the task yet.");
  });
});

describe("the sentence under the name", () => {
  const e = { status: "FAILED", startMs: 0, stopMs: 998_000 };
  test("says how it was started and where it stopped", () => {
    expect(executionWords(e, "order-fulfilment", { version: "2", alias: "PROD" }, { current: null, last: "NotifyCustomer", activity: false }, when)).toBe("Started Nov 8, 2026 07:41:08 on order-fulfilment, PROD, version 2. It ran 16 min 38 s and stopped at NotifyCustomer.");
  });
  test("a run started with the plain ARN is the current revision, and a running one may wait for a worker", () => {
    expect(executionWords({ status: "RUNNING", startMs: 0, stopMs: null }, "m", { version: null, alias: null }, { current: "PackOrder", last: null, activity: true }, when)).toBe("Started Nov 8, 2026 07:41:08 on m, current revision. It is waiting for a worker on PackOrder.");
  });
});

describe("what a failed step teaches", () => {
  const def = { StartAt: "N", States: { N: { Type: "Task", Resource: "arn:aws:states:::lambda:invoke", Retry: [{ ErrorEquals: ["Lambda.ServiceException"], MaxAttempts: 3 }], End: true } } };
  test("a Retry rule that does not match the error", () => {
    expect(retryFor(def.States.N, "KeyError")).toBeNull();
    expect(stepNote(def, { name: "N", status: "failed", error: "KeyError", types: [] }, { status: "FAILED", redrivable: true, pollers: [] })?.text).toBe("The Retry rule on N matches Lambda.ServiceException only, so this error was not retried. Redrive continues from N with the same input, so it would fail the same way until what it calls or the input changes. A redrive is possible for 14 days after the execution ended.");
  });
  test("States.ALL matches every error", () => {
    expect(retryFor({ Retry: [{ ErrorEquals: ["States.ALL"] }] }, "KeyError")).not.toBeNull();
  });
  test("an activity task that waits", () => {
    const wait = { StartAt: "P", TimeoutSeconds: 3600, States: { P: { Type: "Task", Resource: "arn:aws:states:us-east-1:1:activity:pack-order", HeartbeatSeconds: 120, End: true } } };
    expect(stepNote(wait, { name: "P", status: "running", types: [] }, { status: "RUNNING", redrivable: false, pollers: ["fulfilment (py)"] })?.text).toBe("P is an activity task. It stays in progress until a worker calls GetActivityTask for pack-order, then answers with SendTaskSuccess or SendTaskFailure. With HeartbeatSeconds 120 the worker must call SendTaskHeartbeat within that time. fulfilment (py) polls pack-order; the machine's own timeout of 3,600 seconds still applies.");
  });
});

describe("the tense of an activity step's note follows the execution", () => {
  const wait = { StartAt: "P", TimeoutSeconds: 3600, States: { P: { Type: "Task", Resource: "arn:aws:states:us-east-1:1:activity:pack-order", HeartbeatSeconds: 120, End: true } } };
  const note = (status: string) => stepNote(wait, { name: "P", status: "running", types: [] }, { status, redrivable: false, pollers: ["fulfilment (py)"] })?.text;
  test("a run in progress is in the present tense", () => {
    expect(note("RUNNING")).toContain("It stays in progress until a worker calls GetActivityTask for pack-order");
    expect(note("RUNNING")).toContain("the worker must call SendTaskHeartbeat");
    expect(note("RUNNING")).toContain("fulfilment (py) polls pack-order; the machine's own timeout of 3,600 seconds still applies.");
  });
  test("a run that is no longer running says what the step was doing, not what it does", () => {
    for (const [status, how] of [["ABORTED", "was stopped"], ["TIMED_OUT", "timed out"], ["FAILED", "failed"], ["PENDING_REDRIVE", "stopped running"]] as const) {
      const text = note(status)!;
      expect(text).toContain(`It was waiting for a worker to call GetActivityTask for pack-order and answer with SendTaskSuccess or SendTaskFailure when the execution ${how}.`);
      expect(text).toContain("the worker had to call SendTaskHeartbeat");
      expect(text).toContain("fulfilment (py) polled pack-order; the machine's own timeout of 3,600 seconds applied.");
      expect(text).not.toContain("stays in progress");
      expect(text).not.toContain(" polls ");
    }
  });
});

describe("what redriving a failed Lambda step says", () => {
  const state = { Type: "Task", Resource: "arn:aws:states:::lambda:invoke", Parameters: { FunctionName: "send-email", "Payload.$": "$" } };
  const cause = JSON.stringify({ errorType: "KeyError", errorMessage: "'email'" });
  test("a KeyError for a key the input does not have says the input has none", () => {
    const w = redriveWarning({ step: "NotifyCustomer", state, error: "KeyError", cause, input: JSON.stringify({ order: 1 }) });
    expect(w.lead).toBe("The input has no email.");
    expect(w.text).toBe("send-email reads event['email'], so NotifyCustomer fails again unless the function changes. A new execution with the email added would not.");
    expect(causeHint({ state, error: "KeyError", cause, input: JSON.stringify({ order: 1 }) })).toBe("send-email read event['email'] and the input has none");
  });
  test("an input that has the key, a Payload that is not the input, or another error is the general warning", () => {
    const general = redriveWarning({ step: "NotifyCustomer", state, error: "KeyError", cause, input: JSON.stringify({ email: "a@b.c" }) });
    expect(general.lead).toBe("It may fail the same way.");
    expect(general.text).toStartWith("NotifyCustomer stopped with KeyError. Redrive runs it again");
    expect(causeHint({ state: { ...state, Parameters: { FunctionName: "send-email", Payload: { x: 1 } } }, error: "KeyError", cause, input: "{}" })).toBeNull();
    expect(causeHint({ state, error: "ValueError", cause, input: "{}" })).toBeNull();
    expect(causeHint({ state, error: "KeyError", cause: "not json", input: "{}" })).toBeNull();
  });
  test("the teaching line under a failed step names what it calls", () => {
    const note = stepNote({ StartAt: "N", States: { N: state } }, { name: "N", status: "failed", error: "KeyError", types: [] }, { status: "FAILED", redrivable: true, pollers: [] });
    expect(note?.text).toContain("until send-email or the input changes");
    expect(note?.names).toEqual(["N", "send-email"]);
  });
});
