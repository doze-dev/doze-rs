// Secrets Manager's pages, as plain functions: the list's search tokens, a secret as the list reads it, a secret's value as rows, the
// versions and their labels, what rotation says, and the secret the wizard makes.
import { filled, type KeyValueRow } from "@doze/ui/components/app/key-values";
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { andList, cell } from "../cfg-parts";
import { timeMs } from "../model";

// ---- the list ----

export type Prop = "all" | "name" | "description" | "tag-key" | "tag-value";
export const PROPS: { value: Prop; label: string }[] = [
  { value: "all", label: "Any text" },
  { value: "name", label: "Name" },
  { value: "description", label: "Description" },
  { value: "tag-key", label: "Tag key" },
  { value: "tag-value", label: "Tag value" },
];
export type Token = { prop: string; text: string };

/** The tokens as the address keeps them (`name:larder/prod/|tag-key:app`, which ListSecrets' filters are made of). */
export const tokensToQuery = (tokens: readonly Token[]): string => tokens.filter((t) => t.text !== "").map((t) => `${t.prop}:${t.text.replaceAll("|", " ")}`).join("|");
export function tokensFromQuery(q: string): Token[] {
  return q.split("|").flatMap((t) => {
    const i = t.indexOf(":");
    const prop = t.slice(0, i);
    return i > 0 && PROPS.some((p) => p.value === prop) && t.slice(i + 1) !== "" ? [{ prop, text: t.slice(i + 1) }] : [];
  });
}
export const tokenWords = (t: Token): string => `${PROPS.find((p) => p.value === t.prop)?.label ?? t.prop}: ${t.text}`;

export type SecretRow = { name: string; description: string; rotation: boolean; scheduled: string | null; key: string | null };
export function secretRows(rows: readonly ResourceRow[], columns: readonly ColumnSpec[]): SecretRow[] {
  return rows.map((r) => ({ name: r.name, description: cell(columns, r, "Description") ?? "", rotation: cell(columns, r, "Rotation") === "On", scheduled: cell(columns, r, "Marked for deletion"), key: cell(columns, r, "Encryption key") }));
}

/** `2 secrets match 1 filter.` or `3 secrets in us-east-1: 1 with rotation on and 1 scheduled for deletion.` */
export function listSentence(rows: readonly SecretRow[], filtered: boolean, total: number | null): string {
  const n = rows.length;
  const secrets = `${n} secret${n === 1 ? "" : "s"}`;
  if (filtered) return `${secrets} match${n === 1 ? "es" : ""}${total !== null ? ` of ${total}` : ""}.`;
  const on = rows.filter((r) => r.rotation).length;
  const scheduled = rows.filter((r) => r.scheduled).length;
  const parts = [on > 0 && `${on} with rotation on`, scheduled > 0 && `${scheduled} scheduled for deletion`].filter(Boolean) as string[];
  return `${secrets}${parts.length > 0 ? `: ${andList(parts)}` : ""}.`;
}

// ---- the value ----

/** A value that is a JSON object of text, number and true/false values reads as key and value rows; anything else is plaintext. */
export function valueRows(text: string): { key: string; value: string }[] | null {
  try {
    const v = JSON.parse(text) as unknown;
    if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
    const entries = Object.entries(v as Record<string, unknown>);
    return entries.length > 0 && entries.every(([, x]) => ["string", "number", "boolean"].includes(typeof x)) ? entries.map(([key, value]) => ({ key, value: String(value) })) : null;
  } catch {
    return null;
  }
}

/** The rows written as the JSON the console stores: one object of text values. */
export const rowsValue = (rows: readonly KeyValueRow[]): string => JSON.stringify(Object.fromEntries(filled(rows).map((r) => [r.key, r.value])));

export const MAX_VALUE = 65536;
/** What is wrong with a value of `bytes` long, or `null`. */
export const valueProblem = (text: string): string | null => (text === "" ? "Enter a value." : new TextEncoder().encode(text).length > MAX_VALUE ? `A secret holds at most ${MAX_VALUE.toLocaleString("en-US")} bytes.` : null);

// ---- versions ----

export type Version = { id: string; stages: string[]; created: number | null };
export function versions(raw: readonly { VersionId?: string; VersionStages?: string[]; CreatedDate?: number }[]): Version[] {
  return raw.map((v) => ({ id: v.VersionId ?? "", stages: v.VersionStages ?? [], created: v.CreatedDate ? v.CreatedDate * 1000 : null })).sort((a, b) => (b.created ?? 0) - (a.created ?? 0));
}
export const shortId = (id: string): string => id.slice(0, 8);
export const withStage = (vs: readonly Version[], stage: string): Version | undefined => vs.find((v) => v.stages.includes(stage));

/** What saving a new value does to the labels: `9c2e7b1a becomes AWSPREVIOUS and e41b6d08 loses its label.` */
export function savingWords(vs: readonly Version[]): string {
  const cur = withStage(vs, "AWSCURRENT");
  const prev = withStage(vs, "AWSPREVIOUS");
  if (!cur) return "Saving creates a version labeled AWSCURRENT.";
  return `Today that means ${shortId(cur.id)} becomes AWSPREVIOUS${prev ? ` and ${shortId(prev.id)} loses its label` : ""}.`;
}

// ---- rotation and deletion ----

/** `rotate-stripe-key` from `arn:aws:lambda:…:function:rotate-stripe-key`. */
export const functionName = (arn: string): string => arn.split(":").pop() ?? arn;

export type Described = { ARN?: string; Name?: string; Description?: string; KmsKeyId?: string; RotationEnabled?: boolean; RotationLambdaARN?: string; RotationRules?: { AutomaticallyAfterDays?: number; Duration?: string; ScheduleExpression?: string }; LastRotatedDate?: number; NextRotationDate?: number; LastAccessedDate?: number; DeletedDate?: number; Tags?: { Key: string; Value: string }[]; VersionIdsToStages?: Record<string, string[]>; CreatedDate?: number; LastChangedDate?: number };

/** The rotation schedule as written: `rate(30 days)`, `every 30 days`, or none. */
export const scheduleWords = (d: Described): string | null => d.RotationRules?.ScheduleExpression ?? (d.RotationRules?.AutomaticallyAfterDays ? `every ${d.RotationRules.AutomaticallyAfterDays} days` : null);

const DATE = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
export const dayWords = (ms: number): string => DATE.format(new Date(ms));
export const dayOf = (value: string | number | null | undefined): string | null => {
  if (value === null || value === undefined || value === "") return null;
  const ms = typeof value === "number" ? value * 1000 : timeMs(value);
  return ms === null ? null : dayWords(ms);
};

/** What a secret says of its rotation: who last rotated it and when it rotates next. */
export function rotationSentence(d: Described): string | null {
  if (!d.RotationEnabled) return null;
  const fn = d.RotationLambdaARN ? functionName(d.RotationLambdaARN) : null;
  const last = d.LastRotatedDate ? `${fn ?? "Rotation"} last rotated it on ${dayWords(d.LastRotatedDate * 1000)}` : `Rotation is on${fn ? ` with ${fn}` : ""} and has not run yet`;
  const next = d.NextRotationDate ? ` and rotates it next on ${dayWords(d.NextRotationDate * 1000)}` : "";
  return `${last}${next}.`;
}

/** Whether a rotation schedule expression is one ListSecrets-era Secrets Manager takes: `rate(…)` or `cron(…)`. */
export const scheduleProblem = (s: string): string | null => (/^(rate|cron)\(.+\)$/.test(s.trim()) ? null : "Write the schedule as rate(…) or cron(…), in UTC.");
/** A window as hours: `3h`, `90m` is not one. */
export const durationProblem = (s: string): string | null => (s.trim() === "" || /^\d{1,2}h$/.test(s.trim()) ? null : "Write the window in hours, for example 3h.");

// ---- the wizard ----

export type SecretForm = {
  mode: "pairs" | "text";
  pairs: KeyValueRow[];
  text: string;
  key: string | null;
  name: string;
  description: string;
  tags: KeyValueRow[];
  policy: string;
  rotate: boolean;
  schedule: string;
  duration: string;
  fn: string | null;
  now: boolean;
  /** The Regions the secret is copied to (AddReplicaRegions); each is a Region name as typed. */
  replicas: string[];
};
export const blankSecret = (): SecretForm => ({ mode: "pairs", pairs: [], text: "", key: null, name: "", description: "", tags: [], policy: "", rotate: false, schedule: "rate(30 days)", duration: "", fn: null, now: false, replicas: [] });

/** What is wrong with a Region typed for a replica: not a Region name, the Region the secret is in, or one already listed. Empty rows are not wrong (they are dropped). */
export function replicaProblem(region: string, i: number, all: readonly string[], home: string): string | null {
  const r = region.trim();
  if (r === "") return null;
  if (!/^[a-z]{2}(-[a-z]+)+-\d$/.test(r)) return "Write a Region name, for example eu-west-1.";
  if (r === home) return `The secret is already in ${home}.`;
  return all.findIndex((x) => x.trim() === r) < i ? "Another row has this Region." : null;
}
/** The Regions to send, in order, without blanks. */
export const replicaRegions = (all: readonly string[]): string[] => all.map((r) => r.trim()).filter(Boolean);

export const secretValue = (f: SecretForm): string => (f.mode === "pairs" ? rowsValue(f.pairs) : f.text);
export const hasValue = (f: SecretForm): boolean => (f.mode === "pairs" ? filled(f.pairs).length > 0 : f.text !== "");

/** What a name may be: 1 to 512 characters of letters, digits and `/_+=.@-`, not taken. */
export function nameProblem(name: string, taken: readonly string[]): string | null {
  if (name === "") return "Enter a name.";
  if (name.length > 512) return "A name is at most 512 characters.";
  if (!/^[A-Za-z0-9/_+=.@-]+$/.test(name)) return "A name may hold letters, digits and /_+=.@- only.";
  if (taken.includes(name)) return `${name} already exists in this Region.`;
  return null;
}

type Call = { service: string; operation: string; input: Record<string, unknown> };
/** The calls Store makes, in order. The value is masked for the preview, which never needs it. */
export function secretCalls(f: SecretForm, o: { masked: boolean; arn?: string }): Call[] {
  const tags = filled(f.tags);
  const id = f.name;
  const out: Call[] = [{ service: "secretsmanager", operation: "CreateSecret", input: { Name: f.name, ...(f.description ? { Description: f.description } : {}), SecretString: o.masked ? "(masked)" : secretValue(f), ...(f.key ? { KmsKeyId: f.key } : {}), ...(tags.length > 0 ? { Tags: tags.map((t) => ({ Key: t.key, Value: t.value })) } : {}), ...(replicaRegions(f.replicas).length > 0 ? { AddReplicaRegions: replicaRegions(f.replicas).map((Region) => ({ Region })) } : {}) } }];
  if (f.policy.trim() !== "") out.push({ service: "secretsmanager", operation: "PutResourcePolicy", input: { SecretId: id, ResourcePolicy: f.policy } });
  if (f.rotate && f.fn) out.push({ service: "secretsmanager", operation: "RotateSecret", input: { SecretId: id, RotationLambdaARN: f.fn, RotationRules: { ScheduleExpression: f.schedule, ...(f.duration ? { Duration: f.duration } : {}) }, RotateImmediately: f.now } });
  return out;
}

// ---- the schedule builder ----

export type Cadence = { n: string; unit: "days" | "hours" };

/** The builder's cadence of a schedule expression, or null for an expression the builder cannot say (a cron, a rate in minutes). */
export function cadenceOf(expression: string): Cadence | null {
  const m = /^rate\(\s*(\d+)\s+(day|days|hour|hours)\s*\)$/.exec(expression.trim());
  return m ? { n: m[1]!, unit: m[2]!.startsWith("day") ? "days" : "hours" } : null;
}

/** The expression a cadence makes: `rate(30 days)`, and `rate(1 day)` for one. */
export const expressionOf = (c: Cadence): string => `rate(${c.n} ${c.unit === "days" ? (c.n === "1" ? "day" : "days") : c.n === "1" ? "hour" : "hours"})`;

/** What is wrong with a cadence: a whole number of days (1 to 1,000) or of hours (4 or more, as often as every four hours). */
export function cadenceProblem(c: Cadence): string | null {
  if (!/^\d+$/.test(c.n.trim())) return "Enter a whole number.";
  const n = Number(c.n);
  if (c.unit === "days") return n >= 1 && n <= 1000 ? null : "Rotate every 1 to 1,000 days.";
  return n >= 4 ? null : "Rotation can run as often as every 4 hours.";
}
