// The create and update wizards, as pure functions: what a person has chosen (`Wizard`), the calls it makes (ValidateTemplate on step 1,
// GetTemplateSummary on step 2, CreateStack or UpdateStack on the last step, or CreateChangeSet), and what is wrong with it.
import { type KeyValueRow, filled, newRow } from "@doze/ui/components/app/key-values";
import { TAG_RULES } from "../../../kit";
import type { Stack } from "./model";

export type Mode = "create" | "update" | "changeset";

/** What `GetTemplateSummary` says of a template: its parameters, what it needs acknowledged, what it makes. */
export type Declared = { ParameterKey: string; ParameterType?: string; DefaultValue?: string; NoEcho?: boolean; Description?: string; ParameterConstraints?: { AllowedValues?: string[] } };
export type Summary = { Description?: string; Parameters?: Declared[]; ResourceTypes?: string[]; /** Each type that can be imported, with the logical IDs of the resources of it. */ ResourceIdentifierSummaries?: { ResourceType: string; LogicalResourceIds?: string[] }[]; Capabilities?: string[]; CapabilitiesReason?: string };

export type Wizard = {
  mode: Mode;
  /** The stack an update is of. */
  stack: string;
  /** Update: keep the stack's template (UsePreviousTemplate). */
  keepTemplate: boolean;
  source: "url" | "file";
  url: string;
  file: { name: string; text: string } | null;
  name: string;
  values: Record<string, string>;
  /** Update: a parameter that keeps the value the stack has (UsePreviousValue). */
  keep: Record<string, boolean>;
  tags: KeyValueRow[];
  role: string | null;
  failure: "rollback" | "preserve";
  topic: string;
  policy: string;
  timeout: string;
  protect: boolean;
  capabilities: boolean;
};

export function emptyWizard(mode: Mode, stack = ""): Wizard {
  return { mode, stack, keepTemplate: mode !== "create", source: "url", url: "", file: null, name: "", values: {}, keep: {}, tags: [], role: null, failure: "rollback", topic: "", policy: "", timeout: "", protect: false, capabilities: false };
}

/** An update starts from what the stack has: its tags, topics and failure choice. */
export function fromStack(mode: Mode, s: Stack): Wizard {
  return { ...emptyWizard(mode, s.StackName), tags: (s.Tags ?? []).map((t) => newRow(t.Key, t.Value)), topic: s.NotificationARNs?.[0] ?? "", role: s.RoleARN ?? null, failure: s.DisableRollback ? "preserve" : "rollback", protect: Boolean(s.EnableTerminationProtection) };
}

/** The most a template given as text may be: the API's limit for `TemplateBody` (a bigger one is read from an S3 URL). */
export const BODY_MAX = 51_200;

/** What names the template in a call: its S3 URL, its text, or the stack's own (an update that keeps it). */
export function templateRef(w: Wizard): Record<string, unknown> {
  if (w.mode !== "create" && w.keepTemplate) return { UsePreviousTemplate: true };
  if (w.source === "file") return w.file ? { TemplateBody: w.file.text } : {};
  return w.url.trim() ? { TemplateURL: w.url.trim() } : {};
}

/** Why step 1 cannot go on yet, or `null`. */
export function templateProblem(w: Wizard): string | null {
  if (w.mode !== "create" && w.keepTemplate) return null;
  if (w.source === "url") {
    const u = w.url.trim();
    if (u === "") return "Enter the URL of the template.";
    if (u.length > 1024) return "The URL can be at most 1024 characters.";
    if (!/^(https?:\/\/|s3:\/\/)/.test(u)) return "The URL must start with https:// (or s3://).";
    return null;
  }
  if (!w.file) return "Choose a template file.";
  if (new TextEncoder().encode(w.file.text).length > BODY_MAX) return `This file is ${(new TextEncoder().encode(w.file.text).length / 1024).toFixed(1)} KB. A template sent as text can be at most ${BODY_MAX.toLocaleString("en-US")} bytes: put it in an S3 bucket and give its URL.`;
  return null;
}

/** The call step 1 makes (the template read: its syntax checked). `null` when there is no template to read. */
export function validateInput(w: Wizard): Record<string, unknown> | null {
  const ref = templateRef(w);
  return Object.keys(ref).length > 0 && !ref.UsePreviousTemplate ? ref : null;
}

/** The call step 2 makes: the parameters and what the template makes. An update that keeps its template reads the stack's. */
export function summaryInput(w: Wizard): Record<string, unknown> | null {
  if (w.mode !== "create" && w.keepTemplate) return { StackName: w.stack };
  return validateInput(w);
}

const NAME = /^[A-Za-z][A-Za-z0-9-]*$/;

/** What is wrong with a stack name, or `null`. */
export function nameProblem(name: string, taken: readonly string[]): string | null {
  if (name === "") return "Enter a stack name.";
  if (name.length > 128) return "A stack name is at most 128 characters.";
  if (!NAME.test(name)) return "Use only letters, digits and hyphens, starting with a letter.";
  if (taken.includes(name)) return `A stack named ${name} already exists in this Region.`;
  return null;
}

/** A parameter's value as the form holds it: what was typed, else the template's default. */
export const valueOf = (w: Wizard, d: Declared): string => w.values[d.ParameterKey] ?? d.DefaultValue ?? "";

/** What is wrong with a parameter, or `null`. */
export function parameterProblem(w: Wizard, d: Declared): string | null {
  if (w.mode !== "create" && w.keep[d.ParameterKey]) return null;
  const v = valueOf(w, d);
  const allowed = d.ParameterConstraints?.AllowedValues ?? [];
  if (allowed.length > 0 && !allowed.includes(v)) return `Choose one of ${allowed.join(", ")}.`;
  if (v === "" && d.DefaultValue === undefined && !/^(Comma|List)/.test(d.ParameterType ?? "")) return `Enter a value for ${d.ParameterKey}.`;
  if (d.ParameterType === "Number" && v !== "" && Number.isNaN(Number(v))) return `${d.ParameterKey} is a number.`;
  return null;
}

const COUNT = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];

/** What an update says under its tags: that they are the stack's own, or how many are kept. `null` for a stack with no tags. */
export function tagsLine(rows: readonly { key: string; value: string }[], was: readonly { Key: string; Value: string }[], stack: string): string | null {
  if (was.length === 0) return null;
  const kept = was.filter((t) => rows.some((r) => r.key === t.Key && r.value === t.Value)).length;
  const n = was.length <= 10 ? COUNT[was.length]! : String(was.length);
  if (kept === was.length && rows.length === was.length) return was.length === 1 ? `This one matches what ${stack} has.` : `These ${n} match what ${stack} has.`;
  return `${kept} of ${stack}'s ${n} tags ${kept === 1 ? "is" : "are"} kept as ${kept === 1 ? "it is" : "they are"}.`;
}

/** Every parameter of the call, as the board sends it: all of them, a NoEcho one an update keeps as `UsePreviousValue`. */
export function parameterList(w: Wizard, s: Summary | undefined): { ParameterKey: string; ParameterValue?: string; UsePreviousValue?: true }[] {
  return (s?.Parameters ?? []).map((d) => (w.mode !== "create" && w.keep[d.ParameterKey] ? { ParameterKey: d.ParameterKey, UsePreviousValue: true as const } : { ParameterKey: d.ParameterKey, ParameterValue: valueOf(w, d) }));
}

/** Why step 3 cannot go on yet, or `null`. */
export function optionsProblem(w: Wizard, s: Summary | undefined): string | null {
  if (filled(w.tags).length > TAG_RULES.maxRows!) return "A stack can have at most 50 tags.";
  if (filled(w.tags).some((t) => t.key === "" || t.key.length > TAG_RULES.maxKey!)) return "Every tag needs a key of at most 128 characters.";
  if (w.topic.trim() !== "" && !/^arn:aws:sns:[^:]+:\d{12}:[\w.-]+$/.test(w.topic.trim())) return "An SNS topic ARN looks like arn:aws:sns:us-east-1:123456789012:alerts.";
  if (w.policy.trim() !== "") {
    try {
      JSON.parse(w.policy);
    } catch {
      return "The stack policy is not valid JSON.";
    }
  }
  if (w.timeout.trim() !== "" && !/^[1-9]\d{0,5}$/.test(w.timeout.trim())) return "The timeout is a whole number of minutes, 1 or more.";
  if ((s?.Capabilities ?? []).length > 0 && !w.capabilities) return `Acknowledge the capabilities this template needs (${(s?.Capabilities ?? []).join(", ")}).`;
  return null;
}

/** The members the options step adds to a call. `create`: the options only a new stack has (timeout, termination protection). */
export function optionsInput(w: Wizard, s: Summary | undefined, create: boolean): Record<string, unknown> {
  const tags = filled(w.tags);
  const out: Record<string, unknown> = {};
  if (tags.length > 0) out.Tags = tags.map((t) => ({ Key: t.key, Value: t.value }));
  if (w.role) out.RoleARN = w.role;
  if (w.topic.trim()) out.NotificationARNs = [w.topic.trim()];
  out.DisableRollback = w.failure === "preserve";
  if (w.policy.trim()) out.StackPolicyBody = w.policy.trim();
  if (create && w.timeout.trim()) out.TimeoutInMinutes = Number(w.timeout);
  if (create && w.protect) out.EnableTerminationProtection = true;
  if ((s?.Capabilities ?? []).length > 0) out.Capabilities = s?.Capabilities;
  return out;
}

/** The call that makes or changes the stack. */
export function stackInput(w: Wizard, s: Summary | undefined): Record<string, unknown> {
  const create = w.mode === "create";
  return { StackName: create ? w.name : w.stack, ...templateRef(w), ...((s?.Parameters ?? []).length > 0 ? { Parameters: parameterList(w, s) } : {}), ...optionsInput(w, s, create) };
}

/** The call that makes a change set: of a stack to be made (CREATE) or of the stack the wizard is for (UPDATE). */
export function changeSetInput(w: Wizard, s: Summary | undefined, name: string): Record<string, unknown> {
  const rest = stackInput(w, s);
  delete rest.DisableRollback;
  return { ...rest, ChangeSetName: name, ChangeSetType: w.mode === "create" ? "CREATE" : "UPDATE" };
}

/** What is wrong with a change set's name, or `null`. */
/** The first of base, base-2, base-3 ... that is not already a change set's name. */
export function firstFree(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;
  for (let i = 2; ; i++) if (!taken.includes(`${base}-${i}`)) return `${base}-${i}`;
}

export function changeSetNameProblem(name: string, taken: readonly string[]): string | null {
  if (name === "") return "Enter a name for the change set.";
  if (name.length > 128) return "A change set name is at most 128 characters.";
  if (!NAME.test(name)) return "Use only letters, digits and hyphens, starting with a letter.";
  if (taken.includes(name)) return `A change set named ${name} already exists on this stack.`;
  return null;
}

/** A change set the update review makes for its preview: named by the clock, so it is never one the person made. */
export const previewName = (now: number): string => `console-preview-${now}`;

/** A sentence that ends: the text with a full stop when it has none. */
export const dot = (s: string): string => (/[.!?]$/.test(s.trim()) ? s.trim() : `${s.trim()}.`);
