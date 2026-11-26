// The rule wizard's state and what it sends: the draft of a rule and its targets, the rules of its fields (the service's own limits), and the
// PutRule and PutTargets requests. Pure.
import type { KeyValueRow } from "@doze/ui/components/app/key-values";
import { filled } from "@doze/ui/components/app/key-values";
import type { TargetItem } from "./types";
import { arnName } from "./words";

export const DEFAULT_PATTERN = '{\n  "source": ["larder.orders"]\n}';
export const MOST_TARGETS = 5;

/** What kind of thing a target is: one of the services doze-aws delivers to, an API destination, or another event bus. */
export type TargetKind = "sqs" | "sns" | "lambda" | "logs" | "kinesis" | "states" | "api-destination" | "event-bus";
export type InputMode = "event" | "path" | "constant" | "transformer";

export type TargetDraft = {
  key: string;
  kind: TargetKind;
  /** The ARN chosen. */
  arn: string | null;
  input: InputMode;
  path: string;
  constant: string;
  paths: string;
  template: string;
  retry: boolean;
  hours: string;
  minutes: string;
  seconds: string;
  attempts: string;
  dlq: string | null;
};

export type RuleDraft = {
  name: string;
  description: string;
  bus: string;
  kind: "pattern" | "schedule";
  pattern: string;
  /** `rate(…)` or `cron(…)`, as it will be sent. */
  schedule: string;
  enabled: boolean;
  targets: TargetDraft[];
  tags: KeyValueRow[];
};

let n = 0;
export const newTarget = (kind: TargetKind = "sns"): TargetDraft => ({ key: `t${++n}`, kind, arn: null, input: "event", path: "$.detail", constant: "", paths: "", template: "", retry: false, hours: "24", minutes: "0", seconds: "0", attempts: "185", dlq: null });
export const emptyRule = (kind: RuleDraft["kind"] = "pattern", bus = "default"): RuleDraft => ({ name: "", description: "", bus: kind === "schedule" ? "default" : bus, kind, pattern: DEFAULT_PATTERN, schedule: "cron(0 18 ? * MON-FRI *)", enabled: true, targets: [newTarget()], tags: [] });

/** A rule's name: 1 to 64 letters, digits, dot, hyphen and underscore. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Name the rule.";
  if (name.length > 64) return "A rule name is at most 64 characters.";
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : "A rule name takes letters, digits, dot, hyphen and underscore only.";
}

/** The shape PutRule and TestEventPattern check before any condition: a key holds an object (nested) or a non-empty array, and `$or` an array of
 * objects (crates/aws-eventbridge pattern/parse.rs says the same words). `null` when the shape is right. */
function shapeProblem(v: Record<string, unknown>, depth = 0): string | null {
  for (const [key, value] of Object.entries(v)) {
    if (key === "$or") {
      if (!Array.isArray(value)) return "$or value must be an array.";
      if (value.length === 0) return "Empty arrays are not allowed.";
      for (const item of value) {
        if (typeof item !== "object" || item === null || Array.isArray(item)) return "$or must contain only objects.";
        const inner = shapeProblem(item as Record<string, unknown>, depth + 1);
        if (inner) return inner;
      }
    } else if (Array.isArray(value)) {
      if (value.length === 0) return "Empty arrays are not allowed.";
    } else if (typeof value === "object" && value !== null) {
      const inner = shapeProblem(value as Record<string, unknown>, depth + 1);
      if (inner) return inner;
    } else return `"${key}" must be an object or an array.`;
  }
  return null;
}

/** An event pattern is a JSON object of at most 2,048 characters (the default quota). */
export function patternProblem(text: string): string | null {
  if (text.trim() === "") return "Write an event pattern.";
  try {
    const v = JSON.parse(text) as unknown;
    if (typeof v !== "object" || v === null || Array.isArray(v)) return "An event pattern is a JSON object.";
    if (Object.keys(v).length === 0) return "An empty pattern is refused: name at least one field.";
    const shape = shapeProblem(v as Record<string, unknown>);
    if (shape) return shape;
  } catch (e) {
    return `Not JSON: ${(e as Error).message}`;
  }
  return text.replace(/\s/g, "").length > 2048 ? "A pattern can be at most 2,048 characters." : null;
}

/** The seconds a retry policy's age fields make (60 seconds to 24 hours), or why they do not. */
export function ageSeconds(t: Pick<TargetDraft, "hours" | "minutes" | "seconds">): { seconds: number } | { problem: string } {
  const parts = [t.hours, t.minutes, t.seconds].map((x) => (x.trim() === "" ? 0 : Number(x)));
  if (parts.some((p) => !Number.isInteger(p) || p < 0)) return { problem: "The age takes whole numbers." };
  const seconds = parts[0]! * 3600 + parts[1]! * 60 + parts[2]!;
  return seconds < 60 || seconds > 86400 ? { problem: "The maximum age is 60 seconds to 24 hours." } : { seconds };
}

export function attemptsProblem(text: string): string | null {
  const v = Number(text);
  return text.trim() === "" || !Number.isInteger(v) || v < 0 || v > 185 ? "Retry attempts are 0 to 185." : null;
}

/** What is wrong with a target's settings, said where it is; `null` when it can be sent. */
export function targetProblem(t: TargetDraft): string | null {
  if (!t.arn) return "Choose where the target sends.";
  if (t.dlq !== null && t.dlq.endsWith(":")) return "Choose the dead-letter queue.";
  if (t.input === "path" && !/^\$/.test(t.path)) return "A path starts with $.";
  if (t.input === "constant") {
    try {
      JSON.parse(t.constant);
    } catch {
      return "The constant is JSON text.";
    }
  }
  if (t.input === "transformer" && t.template.trim() === "") return "Write the template.";
  if (t.retry) {
    const age = ageSeconds(t);
    if ("problem" in age) return age.problem;
    return attemptsProblem(t.attempts);
  }
  return null;
}

const VERB: Record<TargetKind, string> = { sqs: "send", sns: "publish", lambda: "invoke", logs: "log", kinesis: "put", states: "start", "api-destination": "call", "event-bus": "forward" };

/** The Id a target gets: what it does and to what (`publish-orders-created`), unique among `taken`, at most 64 characters. */
export function targetId(t: Pick<TargetDraft, "kind" | "arn">, taken: readonly string[]): string {
  const base = `${VERB[t.kind]}-${arnName(t.arn ?? "target").replace(/[^.\-_A-Za-z0-9]/g, "-")}`.slice(0, 60);
  let id = base;
  for (let i = 2; taken.includes(id); i++) id = `${base}-${i}`;
  return id;
}

/** The target as PutTargets takes it. */
export function targetOf(t: TargetDraft, id: string): TargetItem {
  const out: TargetItem = { Id: id, Arn: t.arn ?? "" };
  if (t.input === "path") out.InputPath = t.path;
  else if (t.input === "constant") out.Input = t.constant;
  else if (t.input === "transformer") {
    let map: Record<string, string> = {};
    try {
      map = JSON.parse(t.paths || "{}") as Record<string, string>;
    } catch {
      /* the problem is said on the editor */
    }
    out.InputTransformer = { ...(Object.keys(map).length > 0 ? { InputPathsMap: map } : {}), InputTemplate: t.template };
  }
  if (t.retry) {
    const age = ageSeconds(t);
    out.RetryPolicy = { MaximumRetryAttempts: Number(t.attempts), MaximumEventAgeInSeconds: "seconds" in age ? age.seconds : 86400 };
  }
  if (t.dlq) out.DeadLetterConfig = { Arn: t.dlq };
  return out;
}

export const targetsOf = (d: RuleDraft): TargetItem[] => {
  const taken: string[] = [];
  return d.targets.map((t) => {
    const id = targetId(t, taken);
    taken.push(id);
    return targetOf(t, id);
  });
};

/** PutRule's input: the rule with its tags (a rule that exists takes none through PutRule: the tags go through TagResource). */
export function putRuleInput(d: RuleDraft): Record<string, unknown> {
  const tags = filled(d.tags).map((r) => ({ Key: r.key, Value: r.value }));
  return {
    Name: d.name,
    EventBusName: d.bus,
    ...(d.description ? { Description: d.description } : {}),
    ...(d.kind === "pattern" ? { EventPattern: d.pattern } : { ScheduleExpression: d.schedule }),
    State: d.enabled ? "ENABLED" : "DISABLED",
    ...(tags.length > 0 ? { Tags: tags } : {}),
  };
}

export const putTargetsInput = (d: RuleDraft): Record<string, unknown> => ({ Rule: d.name, EventBusName: d.bus, Targets: targetsOf(d) });

/** The first step that cannot be passed, with why; `null` when the draft can be created. Steps are 0-based: detail, pattern (or schedule), targets, tags. */
export function firstProblem(d: RuleDraft, taken: readonly string[]): { step: number; why: string } | null {
  const name = nameProblem(d.name) ?? (taken.includes(d.name) ? `${d.name} is already on ${d.bus}.` : null);
  if (name) return { step: 0, why: name };
  if (d.kind === "pattern") {
    const p = patternProblem(d.pattern);
    if (p) return { step: 1, why: p };
  }
  const first = d.targets.map(targetProblem).find(Boolean);
  if (first) return { step: d.kind === "pattern" ? 2 : 1, why: first };
  return null;
}
