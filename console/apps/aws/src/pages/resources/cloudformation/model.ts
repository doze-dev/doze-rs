// What the CloudFormation pages know of a stack, as pure functions: its status as a colour, the one line a list says of its rows,
// the resources a stack owns as resources of the console, what its template retains, who imports its exports.
import type { Tone } from "@doze/ui/components/app/status-dot";
import type { ResourceLink } from "../../../kit";

/** The members of `DescribeStacks` the pages read. */
export type Stack = {
  StackId: string;
  StackName: string;
  StackStatus: string;
  StackStatusReason?: string;
  Description?: string;
  CreationTime?: number | string;
  LastUpdatedTime?: number | string;
  DeletionTime?: number | string;
  Parameters?: { ParameterKey: string; ParameterValue?: string; ResolvedValue?: string }[];
  Outputs?: { OutputKey: string; OutputValue?: string; Description?: string; ExportName?: string }[];
  Tags?: { Key: string; Value: string }[];
  NotificationARNs?: string[];
  RoleARN?: string;
  Capabilities?: string[];
  EnableTerminationProtection?: boolean;
  DisableRollback?: boolean;
  TimeoutInMinutes?: number;
  ParentId?: string;
  RootId?: string;
  RollbackConfiguration?: { RollbackTriggers?: { Arn: string; Type: string }[]; MonitoringTimeInMinutes?: number };
};

export type StackResource = { LogicalResourceId: string; PhysicalResourceId?: string; ResourceType: string; ResourceStatus: string; ResourceStatusReason?: string; LastUpdatedTimestamp?: number | string };

export type StackEvent = { EventId: string; Timestamp: number | string; LogicalResourceId?: string; ResourceType?: string; ResourceStatus?: string; ResourceStatusReason?: string; PhysicalResourceId?: string };

export const inProgress = (status: string): boolean => /IN_PROGRESS$/.test(status);

/** How a stack's status is coloured: a failed one and a create that was rolled back red, an update that was rolled back (the stack
 * is as it was) amber, work under way blue, a review waiting grey-blue, done green, deleted amber. */
export function stackTone(status: string): Tone {
  if (status === "REVIEW_IN_PROGRESS") return "info";
  if (/FAILED$/.test(status) || status === "ROLLBACK_COMPLETE") return "bad";
  if (/IN_PROGRESS$/.test(status)) return "info";
  if (/ROLLBACK|^DELETE_COMPLETE$|SKIPPED/.test(status)) return "warn";
  if (/COMPLETE$/.test(status)) return "ok";
  return "idle";
}

/** Whether a read failed because CloudFormation has no stack of that name (it answers a deleted stack's name with a 400
 * ValidationError, "Stack with id X does not exist"), as against any other refusal. */
export function isGone(error: unknown): boolean {
  const e = error as { status?: number; code?: string; message?: string } | null;
  return Boolean(e && e.status === 400 && e.code === "ValidationError" && /does not exist/i.test(e.message ?? ""));
}

/** Whether `DescribeStacks` answered with no stack: for a name that is gone the console answers an empty list, not CloudFormation's refusal
 * (so a deleted stack's page makes no failed request). `undefined` (not read yet) is not an answer. */
export const noStack = (answer: { Stacks?: readonly unknown[] } | undefined): boolean => answer !== undefined && (answer.Stacks ?? []).length === 0;

/** The name at the end of a stack id (`arn:aws:cloudformation:r:a:stack/NAME/uuid`). */
export const stackNameOf = (id: string): string => id.split("/")[1] ?? id;

// -- the list ---------------------------------------------------------------------------------------------------------------

export type StackRow = { name: string; status: string; created: string | null; nested: boolean };

/** A list row from the columns of the descriptor: Status, Description, Created, Updated, Parent. */
export const stackRow = (r: { name: string; cells: (string | null)[] }): StackRow => ({ name: r.name, status: r.cells[0] ?? "", created: r.cells[2] ?? null, nested: Boolean(r.cells[4]) });

const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
export const count = (n: number): string => WORDS[n] ?? String(n);

/** Names as a list: `a`, `a and b`, `a, b and c`. */
export function nameList(names: readonly string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

const rolledBack = (s: string) => /ROLLBACK_COMPLETE$/.test(s);

/** The one line a list says: how many stacks and how many nested, which wait for their change set, which rolled back or failed, which
 * are being worked on; the names in it are the stacks'. */
export function listSummary(rows: readonly StackRow[], region: string): { text: string; names: string[] } {
  const n = rows.length;
  if (n === 0) return { text: `No stacks in ${region}.`, names: [] };
  const nested = rows.filter((r) => r.nested).length;
  const head = `${n} ${n === 1 ? "stack" : "stacks"} in ${region}${nested > 0 ? `, ${count(nested)} of them nested` : ""}.`;
  const clauses: string[] = [];
  const names: string[] = [];
  const say = (list: StackRow[], one: string, many: string) => {
    if (list.length === 0) return;
    const ns = list.map((r) => r.name);
    names.push(...ns);
    clauses.push(`${nameList(ns)} ${ns.length === 1 ? one : many}`);
  };
  say(rows.filter((r) => r.status === "REVIEW_IN_PROGRESS"), "is waiting for its change set to be executed", "are waiting for their change sets to be executed");
  say(rows.filter((r) => rolledBack(r.status)), "rolled back", "rolled back");
  say(rows.filter((r) => /_FAILED$/.test(r.status)), "failed", "failed");
  say(rows.filter((r) => inProgress(r.status) && r.status !== "REVIEW_IN_PROGRESS"), "is being worked on", "are being worked on");
  return { text: [head, clauses.length > 0 ? `${clauses.join("; ")}.` : ""].filter(Boolean).join(" "), names };
}

// -- what a stack owns ---------------------------------------------------------------------------------------------------------

const tail = (sep: string) => (physical: string) => physical.split(sep).pop() ?? physical;
const whole = (physical: string) => physical;

/** The resource types a stack may own that the console has a page for: the kind, and the name its page is at. */
const TARGETS: Record<string, [string, string, (physical: string) => string]> = {
  "AWS::SQS::Queue": ["sqs", "queues", tail("/")],
  "AWS::SNS::Topic": ["sns", "topics", tail(":")],
  "AWS::DynamoDB::Table": ["dynamodb", "tables", whole],
  "AWS::S3::Bucket": ["s3", "buckets", whole],
  "AWS::Lambda::Function": ["lambda", "functions", whole],
  "AWS::Logs::LogGroup": ["logs", "groups", whole],
  "AWS::Kinesis::Stream": ["kinesis", "streams", whole],
  "AWS::IAM::Role": ["iam", "roles", whole],
  "AWS::StepFunctions::StateMachine": ["stepfunctions", "machines", tail(":")],
  "AWS::CloudWatch::Alarm": ["cloudwatch", "alarms", whole],
  "AWS::SSM::Parameter": ["ssm", "parameters", whole],
};

/** Where the console has this resource's page, when it has one; `null` for a type it has none for and for a resource not made yet. */
export function resourceTarget(r: Pick<StackResource, "ResourceType" | "PhysicalResourceId">): ResourceLink | null {
  const t = TARGETS[r.ResourceType];
  if (!t || !r.PhysicalResourceId) return null;
  return { service: t[0], kind: t[1], name: t[2](r.PhysicalResourceId) };
}

/** The name a resource goes by on its own page (the tail of a queue's URL), else its physical id. */
export const physicalName = (r: Pick<StackResource, "ResourceType" | "PhysicalResourceId">): string => resourceTarget(r)?.name ?? r.PhysicalResourceId ?? "";

/** How many services the resources are of (`AWS::SQS::Queue` and `AWS::SNS::Topic` are two). */
export function serviceCount(resources: readonly StackResource[]): number {
  return new Set(resources.map((r) => r.ResourceType.split("::")[1] ?? r.ResourceType)).size;
}

/** The resources that are still the stack's to delete (a DELETE_COMPLETE one is gone). */
export const owned = (resources: readonly StackResource[]): StackResource[] => resources.filter((r) => r.ResourceStatus !== "DELETE_COMPLETE");

// -- what a template retains ---------------------------------------------------------------------------------------------------

export type Retained = { id: string; type: string };

const keeps = (policy: string): boolean => ["Retain", "RetainExceptOnCreate"].includes(policy.replace(/^["']|["']$/g, ""));

/** The resources whose `DeletionPolicy` retains them, from a template as the service returned it (JSON, or YAML read line by line for
 * the logical ids under `Resources:`; nothing is evaluated, so a policy a parameter sets is not seen). The same reading as the
 * server's delete confirmation (crates/aws-console/src/resources/cfn_kept.rs). */
export function retainedIn(template: string): Retained[] {
  try {
    const t = JSON.parse(template) as { Resources?: Record<string, { Type?: string; DeletionPolicy?: string }> };
    return Object.entries(t.Resources ?? {}).filter(([, r]) => typeof r.DeletionPolicy === "string" && keeps(r.DeletionPolicy)).map(([id, r]) => ({ id, type: r.Type ?? "" }));
  } catch {
    /* YAML */
  }
  let inResources = false;
  let idsAt: number | null = null;
  const done: { id: string; type: string; keep: boolean }[] = [];
  let current: { id: string; type: string; keep: boolean } | null = null;
  for (const line of template.split("\n")) {
    const body = line.trimStart();
    if (body === "" || body.startsWith("#")) continue;
    const indent = line.length - body.length;
    if (indent === 0) {
      if (current) done.push(current);
      current = null;
      inResources = body.startsWith("Resources:");
      idsAt = null;
      continue;
    }
    const colon = body.indexOf(":");
    if (!inResources || colon < 0) continue;
    const key = body.slice(0, colon).replace(/^["']|["']$/g, "");
    const value = body.slice(colon + 1).split("#")[0]?.trim() ?? "";
    idsAt ??= indent;
    if (idsAt === indent) {
      if (current) done.push(current);
      current = { id: key, type: "", keep: false };
    } else if (current) {
      if (key === "Type") current.type = value;
      else if (key === "DeletionPolicy") current.keep = keeps(value);
    }
  }
  if (current) done.push(current);
  return done.filter((r) => r.keep).map(({ id, type }) => ({ id, type }));
}

// -- the delete -----------------------------------------------------------------------------------------------------------------

/** What deleting a stack does, split by the template's `DeletionPolicy`: the resources deleted and the ones kept. */
export function deletionSplit(resources: readonly StackResource[], retained: readonly Retained[]): { deleted: StackResource[]; kept: StackResource[] } {
  const keep = new Set(retained.map((r) => r.id));
  const mine = owned(resources);
  return { deleted: mine.filter((r) => !keep.has(r.LogicalResourceId)), kept: mine.filter((r) => keep.has(r.LogicalResourceId)) };
}

/** Why a delete would not be carried out, in words, or `null` when nothing stands in its way. */
export function deleteRefusal(stack: Pick<Stack, "EnableTerminationProtection" | "StackName">, importers: Readonly<Record<string, readonly string[]>>): string | null {
  if (stack.EnableTerminationProtection) return `Termination protection is on for ${stack.StackName}, so DeleteStack is refused. Turn it off first.`;
  const names = [...new Set(Object.values(importers).flat())];
  if (names.length > 0) return `${nameList(names)} ${names.length === 1 ? "imports" : "import"} ${Object.entries(importers).filter(([, v]) => v.length > 0).map(([k]) => k).join(", ")}, so DeleteStack leaves ${stack.StackName} as it is.`;
  return null;
}

// -- the events ------------------------------------------------------------------------------------------------------------------

/** The resource whose create failed first (events come newest first; a rollback then fails and deletes the rest); `null` when none did. */
export function failedResource(events: readonly Partial<StackEvent>[]): { resource: string; type: string; reason: string } | null {
  const failed = events.filter((e) => e.ResourceStatus === "CREATE_FAILED" && e.ResourceType !== "AWS::CloudFormation::Stack" && e.LogicalResourceId);
  const first = failed[failed.length - 1];
  return first?.LogicalResourceId ? { resource: first.LogicalResourceId, type: first.ResourceType ?? "", reason: first.ResourceStatusReason ?? "" } : null;
}

/** The newest record of a deleted stack of this name among `ListStacks`'s `StackSummaries`; `null` when there is none. */
export function deletedRecord(summaries: readonly { StackName?: string; StackId?: string; DeletionTime?: number | string; CreationTime?: number | string }[], name: string): { id: string; deleted: string } | null {
  const time = (s: { DeletionTime?: number | string }) => Number(s.DeletionTime) || Date.parse(String(s.DeletionTime)) || 0;
  const newest = summaries.filter((s) => s.StackName === name && s.StackId).sort((a, b) => time(b) - time(a))[0];
  return newest?.StackId ? { id: newest.StackId, deleted: String(newest.DeletionTime ?? "") } : null;
}
