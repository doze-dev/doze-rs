// What Workflow Studio's Config mode holds and the calls it makes from it: CreateStateMachine and UpdateStateMachine bodies,
// the role the studio makes, and why a draft cannot be created yet. Pure, so it is tested without a browser.
import { logGroupArn, logGroupOf, nameProblem, roleNameOf } from "./model";

export type Where = { region: string; account: string };
export type RoleChoice = "create" | "existing" | "arn";
export type Level = "OFF" | "ALL" | "ERROR" | "FATAL";
export type Tag = { key: string; value: string };

export type Config = {
  type: "STANDARD" | "EXPRESS";
  role: RoleChoice;
  /** The role picked (`existing`) or typed (`arn`), as an ARN. */
  roleArn: string;
  level: Level;
  data: boolean;
  tracing: boolean;
  publish: boolean;
  kms: boolean;
  kmsKey: string | null;
  kmsPeriod: string;
  tagsOn: boolean;
  tags: Tag[];
};

export const FIRST_CONFIG: Config = { type: "STANDARD", role: "create", roleArn: "", level: "OFF", data: false, tracing: false, publish: false, kms: false, kmsKey: null, kmsPeriod: "300", tagsOn: false, tags: [{ key: "", value: "" }] };

/** The blank canvas's definition: AWS's Hello World, a Pass state that ends. */
export const BLANK = { Comment: "A description of my state machine", StartAt: "Hello", States: { Hello: { Type: "Pass", Result: "Hello, World!", End: true } } };

/** The ARN of the role the studio makes for `name`. */
export const madeRoleArn = (name: string, w: Where): string => `arn:aws:iam::${w.account}:role/${roleNameOf(name)}`;

/** The role the machine is created with. */
export const roleOf = (name: string, c: Config, w: Where): string => (c.role === "create" ? madeRoleArn(name, w) : c.roleArn.trim());

const periodOf = (c: Config): number => Number(c.kmsPeriod);

/** Why the draft cannot be created yet, in a sentence; `null` when it can. */
export function createProblem(name: string, c: Config, valid: boolean, taken: readonly string[]): string | null {
  const n = nameProblem(name, taken);
  if (n) return n;
  if (!valid) return "The definition is invalid: fix it first.";
  if (c.role !== "create" && !/^arn:aws[a-z-]*:iam::\d{12}:role\/.+/.test(c.roleArn.trim())) return c.role === "existing" ? "Choose an execution role." : "Enter the role's ARN (arn:aws:iam::<account>:role/<name>).";
  if (c.kms && (!c.kmsKey || !(periodOf(c) >= 60 && periodOf(c) <= 900))) return c.kmsKey ? "The data key reuse period is 60 to 900 seconds." : "Choose a KMS key.";
  if (c.tagsOn && c.tags.some((t) => t.key.trim() === "" && t.value !== "")) return "A tag needs a key.";
  return null;
}

/** The logging block: nothing for OFF, the level and the log group Step Functions names by default otherwise. */
export function loggingOf(name: string, c: Config, w: Where): object {
  if (c.level === "OFF") return { level: "OFF" };
  return { level: c.level, includeExecutionData: c.data, destinations: [{ cloudWatchLogsLogGroup: { logGroupArn: logGroupArn(name, w) } }] };
}

/** The `encryptionConfiguration` of a customer managed key, or nothing: Step Functions keeps it (doze-aws returns it and encrypts nothing). */
export const encryptionOf = (c: Pick<Config, "kms" | "kmsKey" | "kmsPeriod">) => (c.kms ? { encryptionConfiguration: { type: "CUSTOMER_MANAGED_KMS_KEY", kmsKeyId: c.kmsKey, kmsDataKeyReusePeriodSeconds: Number(c.kmsPeriod) } } : {});

/** Why a key chosen for encryption is not enough yet; `null` when it is. */
export const encryptionProblem = (c: Pick<Config, "kms" | "kmsKey" | "kmsPeriod">): string | null => (!c.kms ? null : !c.kmsKey ? "Choose a KMS key." : Number(c.kmsPeriod) >= 60 && Number(c.kmsPeriod) <= 900 ? null : "The data key reuse period is 60 to 900 seconds.");

/** The body of CreateStateMachine as the Config mode shows it (the definition as the text the person has). */
export function createBody(name: string, definition: string, c: Config, w: Where): Record<string, unknown> {
  const tags = c.tagsOn ? c.tags.filter((t) => t.key.trim() !== "").map((t) => ({ key: t.key.trim(), value: t.value })) : [];
  return {
    name,
    type: c.type,
    definition,
    roleArn: roleOf(name, c, w),
    loggingConfiguration: loggingOf(name, c, w),
    tracingConfiguration: { enabled: c.tracing },
    ...encryptionOf(c),
    ...(c.publish ? { publish: true } : {}),
    ...(tags.length > 0 ? { tags } : {}),
  };
}

/** The body of UpdateStateMachine (the machine is named by the page). */
export function updateBody(name: string, definition: string, c: Config, w: Where): Record<string, unknown> {
  return {
    definition,
    roleArn: c.roleArn.trim(),
    loggingConfiguration: loggingOf(name, c, w),
    tracingConfiguration: { enabled: c.tracing },
    ...encryptionOf(c),
  };
}

/** What a log level lets through, said as the board says it. */
export const LEVEL_WORDS: Record<Level, string> = { OFF: "OFF", ALL: "ALL", ERROR: "ERROR", FATAL: "FATAL" };

export { logGroupOf };

/** One diagnostic of ValidateStateMachineDefinition. */
export type Diagnostic = { severity: "ERROR" | "WARNING"; code: string; message: string; location?: string };
export type Validation = { result: "OK" | "FAIL"; diagnostics: Diagnostic[]; truncated?: boolean };

/** The first error of a validation, in the board's words: `QueueReorder` pointed at a state that does not exist. */
export function diagnosticWords(v: Validation | undefined): string | null {
  const d = v?.diagnostics.find((x) => x.severity === "ERROR");
  return d ? wordsOf(d) : null;
}

/** One diagnostic as a sentence: a transition to a state that is not there says which state points where; the rest keep the validator's message. */
export function wordsOf(d: Diagnostic): string {
  const target = /Missing '(\w+)' target: (.+)$/.exec(d.message);
  const from = d.location ? /^\/States\/([^/]+)\/(\w+)/.exec(d.location) : null;
  if (target && from) return `The definition below points ${decodeURIComponent(from[1] ?? "")} at a state that doesn't exist: ${target[1]} names ${target[2]}.`;
  return d.location && d.location !== "null" ? `${d.message} (at ${d.location})` : d.message;
}

/** A diagnostic that only follows from another: a definition whose transition points nowhere has "no terminal state" too, and that
 * says nothing the first problem does not. */
const consequential = (d: Diagnostic, all: readonly Diagnostic[]): boolean => d.code === "MISSING_END_STATE" && all.some((x) => x.code === "MISSING_TRANSITION_TARGET");

/** The problems of a definition past the first, which the page's sentence already says, without the ones that only follow from another. */
export function furtherProblems(v: Validation | undefined): Diagnostic[] {
  const all = v?.diagnostics ?? [];
  const first = all.find((d) => d.severity === "ERROR");
  return all.filter((d) => d !== first && !consequential(d, all));
}

/** The lines of the text the errors name (1-based, each once): the lines the editor draws in the link colour. */
export function problemLines(text: string, v: Validation | undefined): number[] {
  const all = v?.diagnostics ?? [];
  const lines = all.filter((d) => d.severity === "ERROR" && !consequential(d, all)).map((d) => lineOfLocation(text, d.location));
  return [...new Set(lines.filter((n): n is number => n !== null))];
}

/** The line (1-based) of `text` a diagnostic's location (`/States/QueueReorder/Next`) is on, found by walking its keys down the text;
 * `null` when it cannot be found. */
export function lineOfLocation(text: string, location: string | undefined): number | null {
  if (!location || location === "null") return null;
  const keys = location.split("/").filter((k) => k !== "" && !/^\d+$/.test(k)).map((k) => k.replace(/~1/g, "/").replace(/~0/g, "~"));
  const rows = text.split("\n");
  let at = 0;
  let found: number | null = null;
  for (const k of keys) {
    // The key itself (`"QueueReorder":`), not a line that only names it as a value (`"Next": "QueueReorder"`).
    const key = new RegExp(`"${k.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}"\\s*:`);
    const i = rows.findIndex((r, n) => n >= at && key.test(r));
    if (i < 0) break;
    found = i + 1;
    at = i;
  }
  return found;
}
