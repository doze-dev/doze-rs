// What the CloudWatch Logs pages say, as pure functions: a log group's row and its words, the sentences of the list and of a group's
// tabs, the rules of the forms' fields (the service's own limits), and the requests the forms make.
import type { FieldValue } from "../../../api/types/FieldValue";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import type { SubList } from "../../../api/types/SubList";
import { timeMs } from "../model";
import { pluralWord } from "@doze/ui/lib/format";

/** The retention periods, in days, that CloudWatch Logs accepts (PutRetentionPolicy). */
export const RETENTIONS = [1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1096, 1827, 2192, 2557, 2922, 3288, 3653] as const;

/** `30 days`, `1 day`, `Never Expire` (no retention policy). */
export const retentionWords = (days: number | null): string => (days === null ? "Never Expire" : days === 1 ? "1 day" : `${days} days`);

/** A count with its noun: `2 metric filters`. */
export const count = (n: number, one: string, many = pluralWord(one)): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** The class's words: Infrequent Access is Essentials on AWS now. */
export const classWords = (cls: string | null): string => (cls === "INFREQUENT_ACCESS" ? "Essentials" : "Standard");

/** A log group as its list row says it (the descriptor's columns: class, retention, metric filters, created). */
export type GroupRow = { name: string; cls: string | null; retention: number | null; filters: number; created: string | null };

export function groupRows(rows: readonly ResourceRow[]): GroupRow[] {
  return rows.map((r) => {
    const days = Number(r.cells[1]);
    return { name: r.name, cls: r.cells[0], retention: r.cells[1] !== null && r.cells[1] !== "" && Number.isFinite(days) ? days : null, filters: Number(r.cells[2] ?? 0) || 0, created: r.cells[3] };
  });
}

/** The line over the list: how many, one group with its filters and its retention, and where to change them. */
export function listSentence(rows: readonly GroupRow[]): string {
  if (rows.length === 0) return "";
  const head = `${count(rows.length, "log group")}.`;
  const withFilters = rows.reduce<GroupRow | null>((best, g) => (g.filters > (best?.filters ?? 0) ? g : best), null);
  const kept = rows.find((g) => g.retention !== null);
  const one = withFilters ?? kept;
  const about = one
    ? ` ${one.name}${withFilters ? ` has ${count(withFilters.filters, "metric filter")} and` : ""} ${one.retention === null ? "has no retention policy" : `keeps events for ${retentionWords(one.retention)}`}.`
    : " None has a retention policy.";
  const where = ` Choose a retention in the Retention column to change it${rows.some((g) => g.filters > 0) ? ", or the number in the Metric filters column to see a group's filters" : ""}.`;
  return `${head}${about}${where}`;
}

/** The label's value among a detail's fields. */
export const fieldOf = (fields: readonly FieldValue[], label: string): string | null => fields.find((f) => f.label === label)?.value ?? null;

/** A log group as its page says it. */
export type Group = { name: string; arn: string; cls: string | null; retention: number | null; stored: number | null; filters: number; kms: string | null; created: string | null };

export function groupOf(name: string, fields: readonly FieldValue[]): Group {
  const days = Number(fieldOf(fields, "Retention (days)"));
  const stored = Number(fieldOf(fields, "Stored"));
  return {
    name,
    arn: fieldOf(fields, "ARN") ?? "",
    cls: fieldOf(fields, "Class"),
    retention: Number.isFinite(days) && fieldOf(fields, "Retention (days)") !== "Never expire" && fieldOf(fields, "Retention (days)") !== null ? days : null,
    stored: Number.isFinite(stored) && fieldOf(fields, "Stored") !== null ? stored : null,
    filters: Number(fieldOf(fields, "Metric filters") ?? 0) || 0,
    kms: fieldOf(fields, "KMS key") === "None" ? null : fieldOf(fields, "KMS key"),
    created: fieldOf(fields, "Created"),
  };
}

/** The ARN the board shows: the one AWS gives a group in DescribeLogGroups ends in `:*`. */
export const arnWithStar = (arn: string): string => (arn.endsWith(":*") || arn === "" ? arn : `${arn}:*`);

/** A row of a list of a group's page, read by its cell. */
export type Stream = { name: string; last: number | null };
export function streamsOf(sub: SubList | undefined): Stream[] {
  return (sub?.rows ?? []).map((r) => ({ name: r.name, last: r.cells[1] ? timeMs(r.cells[1]) : null }));
}

/** A metric filter of a group: what DescribeMetricFilters says of it. */
export type MetricFilter = { name: string; pattern: string; metric: string; namespace: string; value: string; unit: string };
export function filtersOf(sub: SubList | undefined): MetricFilter[] {
  return (sub?.rows ?? []).map((r) => {
    const c = (i: number) => r.cells[i] ?? "";
    return { name: c(0), pattern: c(1), metric: c(2), namespace: c(3), value: c(4), unit: c(5) };
  });
}

/** A subscription filter of a group. */
export type SubscriptionFilter = { name: string; pattern: string; destination: string };
export function subscriptionsOf(sub: SubList | undefined): SubscriptionFilter[] {
  return (sub?.rows ?? []).map((r) => ({ name: r.cells[0] ?? r.name, pattern: r.cells[1] ?? "", destination: r.cells[2] ?? "" }));
}

/** The last hour's events of a group, said: how many, from how many streams, and where the newest is. */
export function activityWords(name: string, hour: { events: number; streams: number; more: boolean } | null, newest: { at: string; stream: string } | null, streams: number): string {
  if (streams === 0) return `${name} is empty. Nothing has written to it yet.`;
  const took = hour === null ? "" : hour.events === 0 ? `${name} took no events in the last hour.` : `${name} took ${hour.events.toLocaleString("en-US")}${hour.more ? "+" : ""} ${hour.events === 1 ? "event" : "events"} in the last hour from ${count(hour.streams, "stream")}.`;
  const last = newest ? `${took ? " " : `${name} has ${count(streams, "stream")}. `}The newest event arrived at ${newest.at} in ${newest.stream}.` : "";
  return `${took}${last}`.trim();
}

// The rules of the forms' fields, in the service's own numbers.

/** A log group's name (CreateLogGroup): 1 to 512 of letters, digits and `_-/.#`, and not beginning `aws/`. */
export function groupNameProblem(name: string): string | null {
  if (name === "") return "Enter a name.";
  if (name.length > 512) return "A name is 1 to 512 characters.";
  if (!/^[.\-_/#A-Za-z0-9]+$/.test(name)) return "Use letters, digits and _ - / . # only.";
  if (name.startsWith("aws/")) return "A name cannot begin with aws/: AWS keeps those for its own services.";
  return null;
}

/** A KMS key's ARN, when one is given. */
export const kmsProblem = (arn: string): string | null => (arn.trim() === "" || /^arn:[a-z0-9-]+:kms:[a-z0-9-]+:\d{12}:key\/[\w-]+$/.test(arn.trim()) ? null : "Give the key's ARN: arn:aws:kms:<region>:<account>:key/<id>.");

/** A filter's name (PutMetricFilter, PutSubscriptionFilter): 1 to 512 characters, no `:` or `*`. */
export function filterNameProblem(name: string): string | null {
  if (name.trim() === "") return "Enter a name.";
  if (name.length > 512) return "A name is up to 512 characters.";
  if (/[:*]/.test(name)) return "A name cannot contain : or *.";
  return null;
}

/** A metric name (PutMetricFilter): up to 255 characters, no `:`, `*` or `$`. */
export function metricNameProblem(name: string): string | null {
  if (name.trim() === "") return "Enter a metric name.";
  if (name.length > 255) return "A metric name is up to 255 characters.";
  if (/[:*$]/.test(name)) return "A metric name cannot contain : * or $.";
  return null;
}

/** A metric namespace: letters, digits and `.-_/#:`, up to 255. */
export function namespaceProblem(ns: string): string | null {
  if (ns.trim() === "") return "Enter a namespace.";
  if (ns.length > 255) return "A namespace is up to 255 characters.";
  if (!/^[.\-_/#A-Za-z0-9:]+$/.test(ns)) return "Use letters, digits and . - _ / # : only.";
  return null;
}

/** A metric value: a number, or a `$.field` / `$field` reference. */
export const valueProblem = (v: string): string | null => (/^\s*(-?\d+(\.\d+)?|\$[.\w[\]]+)\s*$/.test(v) ? null : "Enter a number, or a field reference like $.latency.");

/** The units CloudWatch accepts for a metric filter. */
export const UNITS = ["Seconds", "Microseconds", "Milliseconds", "Bytes", "Kilobytes", "Megabytes", "Gigabytes", "Terabytes", "Bits", "Kilobits", "Megabits", "Gigabits", "Terabits", "Percent", "Count", "Bytes/Second", "Kilobytes/Second", "Megabytes/Second", "Gigabytes/Second", "Terabytes/Second", "Bits/Second", "Kilobits/Second", "Megabits/Second", "Gigabits/Second", "Terabits/Second", "Count/Second", "None"] as const;

/** What a pattern of up to 1,024 characters may be: the console only checks the length, the service parses the rest. */
export const patternProblem = (p: string): string | null => (p.length > 1024 ? "A pattern is up to 1,024 characters." : null);

/** The test events of the metric filter's first step: one message per line, up to 50. */
export const messagesOf = (text: string): string[] => text.split("\n").map((l) => l.trim()).filter((l) => l !== "");

// The requests.

export type CreateForm = { name: string; cls: "STANDARD" | "INFREQUENT_ACCESS"; kms: string; protect: boolean };
export const emptyCreate: CreateForm = { name: "", cls: "STANDARD", kms: "", protect: false };

/** CreateLogGroup's input: the class always, the key and the protection only when asked for. */
export function createInput(f: CreateForm): Record<string, unknown> {
  return { logGroupName: f.name, logGroupClass: f.cls, ...(f.kms.trim() ? { kmsKeyId: f.kms.trim() } : {}), ...(f.protect ? { deletionProtectionEnabled: true } : {}) };
}

export type MetricForm = { name: string; namespace: string; metric: string; value: string; fallback: string; unit: string };
export const emptyMetric: MetricForm = { name: "", namespace: "", metric: "", value: "1", fallback: "", unit: "Count" };

/** PutMetricFilter's input (the group is the address's): one transformation; the default value and the unit only when set. */
export function metricInput(group: string, pattern: string, f: MetricForm): Record<string, unknown> {
  return {
    logGroupName: group,
    filterName: f.name,
    filterPattern: pattern,
    metricTransformations: [{ metricNamespace: f.namespace, metricName: f.metric, metricValue: f.value, ...(f.fallback.trim() !== "" ? { defaultValue: Number(f.fallback) } : {}), ...(f.unit && f.unit !== "None" ? { unit: f.unit } : {}) }],
  };
}

/** PutSubscriptionFilter's input: the pattern may be empty (every event). */
export const subscriptionInput = (group: string, name: string, pattern: string, destination: string): Record<string, unknown> => ({ logGroupName: group, filterName: name, filterPattern: pattern, destinationArn: destination });

/** The ARN of a Lambda function. */
export const functionArn = (region: string, account: string, name: string): string => `arn:aws:lambda:${region}:${account}:function:${name}`;

/** Whether a function writes to this very group: subscribing it would invoke it with every line it writes. */
export const isOwnGroup = (group: string, fn: string): boolean => group === `/aws/lambda/${fn}`;

/** The namespaces and metric names the other filters already hold, for the hint under the namespace. */
export function holding(filters: readonly MetricFilter[], namespace: string): string[] {
  return [...new Set(filters.filter((f) => f.namespace === namespace).map((f) => f.metric))];
}

/** Search results newest first, as the board lists them (FilterLogEvents answers oldest first); events with the same time keep their order. */
export const newestFirst = <T extends { timestamp?: number }>(events: readonly T[]): T[] => events.map((e, i) => ({ e, i })).sort((a, b) => (b.e.timestamp ?? 0) - (a.e.timestamp ?? 0) || a.i - b.i).map((x) => x.e);
