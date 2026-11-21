// What a stack's page says of where the stack stands, built from what was read of it: its status, its resources, its exports and
// who imports them, and (for a failed create) the resource that failed. Each returns a sentence whose resource names the page sets
// in bold type, with the list of those names.
import { count, nameList, type StackResource } from "./model";

export type Said = { text: string; names: string[] };

export type StackFacts = {
  name: string;
  status: string;
  reason?: string;
  /** The times as the page says them (`2026-11-06 18:40:12 UTC`). */
  created: string | null;
  updated: string | null;
  deleted?: string | null;
  resources: readonly StackResource[];
  /** The names of the exports its outputs make. */
  exports: readonly string[];
  /** Who imports what: the stacks that import each export. */
  importers: Readonly<Record<string, readonly string[]>>;
  /** The resource whose create failed first, when one did. */
  failed: { resource: string; type: string; reason: string } | null;
  /** Its change set, for a stack waiting for one. */
  waiting?: { name: string; adds: { logical: string; type: string }[] } | null;
  /** Its resources the stack keeps when it goes. */
  retained?: readonly string[];
};

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The one sentence under a stack's name. */
export function stackSaid(f: StackFacts): Said {
  const { name, status } = f;
  const names = [name];
  const since = f.updated ?? f.created ?? "";
  if (status === "REVIEW_IN_PROGRESS") {
    const w = f.waiting;
    const adds = w ? ` Its change set ${w.name} would add ${plural(w.adds.length, "resource")}: ${nameList(w.adds.map((a) => `${a.logical} (${typeWords(a.type)})`))}.` : "";
    return { text: `${name} is waiting for review.${adds}`, names: [name, ...(w ? [w.name, ...w.adds.map((a) => a.logical)] : [])] };
  }
  if (status === "ROLLBACK_COMPLETE") {
    const why = f.failed ? ` ${f.failed.resource} could not be made${f.failed.reason ? `: ${dot(plainReason(f.failed.reason))}` : "."}` : "";
    return { text: `${name} failed to create and CloudFormation undid it.${why} It holds no resources.`, names: f.failed ? [name, f.failed.resource] : names };
  }
  if (status === "CREATE_FAILED") {
    const why = f.failed ? ` ${f.failed.resource} could not be made${f.failed.reason ? `: ${dot(plainReason(f.failed.reason))}` : "."}` : "";
    return { text: `${name} failed to create and was not rolled back, so what was made before the failure is still there.${why}`, names: f.failed ? [name, f.failed.resource] : names };
  }
  if (status === "UPDATE_ROLLBACK_COMPLETE") return { text: `${name} was rolled back: its last update failed, and the stack holds what it held before that update.${f.reason ? ` Reason: ${f.reason}` : ""}`, names };
  if (/FAILED$/.test(status)) return { text: `${name} is in ${status}: the last operation did not finish.${f.reason ? ` Reason: ${f.reason}` : ""}`, names };
  if (/IN_PROGRESS$/.test(status)) return { text: `${name} is being worked on (${status}). This page reads it again until it settles.`, names };
  if (status === "DELETE_COMPLETE") {
    const kept = f.retained ?? [];
    return { text: `${name} is gone${f.resources.length > 0 ? " and what it owned with it" : ""}.${kept.length > 0 ? ` ${nameList(kept)} ${kept.length === 1 ? "was" : "were"} kept, as its template asked.` : ""}`, names: [name, ...kept] };
  }
  const own = f.resources.filter((r) => r.ResourceStatus !== "DELETE_COMPLETE");
  const services = new Set(own.map((r) => r.ResourceType.split("::")[1])).size;
  const stable = since ? ` and has been stable since ${since}` : "";
  const owns = `${name} owns ${plural(own.length, "resource")} in ${plural(services, "service")}${stable}.`;
  const used = Object.entries(f.importers).filter(([, v]) => v.length > 0);
  const users = [...new Set(used.flatMap(([, v]) => v))];
  const exp = f.exports.length;
  const end =
    exp === 0 ? ""
    : users.length === 0 ? ` No stack imports its ${exp === 1 ? "export" : `${count(exp)} exports`}, so nothing stops you deleting it.`
    : ` ${nameList(users)} ${users.length === 1 ? "imports" : "import"} ${nameList(used.map(([k]) => k))}, so ${name} can't be deleted while ${users.length === 1 ? "it does" : "they do"}.`;
  return { text: `${owns}${end}`, names: [name, ...users] };
}

/** What a stack that has been deleted says: the whole of it, from its record. */
export function deletedSaid(f: StackFacts): Said {
  return stackSaid({ ...f, status: "DELETE_COMPLETE" });
}

/** What the page says after an update: the resources the last update made or changed, read from its events (newest first, up to the
 * stack's own UPDATE_IN_PROGRESS that began it). A resource the update left alone has no event. */
export function updatedSaid(name: string, events: readonly { LogicalResourceId?: string; ResourceType?: string; ResourceStatus?: string }[]): Said {
  const start = events.findIndex((e) => e.ResourceType === "AWS::CloudFormation::Stack" && e.ResourceStatus === "UPDATE_IN_PROGRESS");
  const last = start < 0 ? events : events.slice(0, start);
  const of = (status: string) => [...new Set(last.filter((e) => e.ResourceStatus === status && e.ResourceType !== "AWS::CloudFormation::Stack" && e.LogicalResourceId).map((e) => e.LogicalResourceId as string))].reverse();
  const made = of("CREATE_COMPLETE");
  const changed = of("UPDATE_COMPLETE");
  const was = (l: readonly string[]) => (l.length === 1 ? "was" : "were");
  const parts = [made.length > 0 && `${nameList(made)} ${was(made)} created`, changed.length > 0 && `${nameList(changed)} ${was(changed)} changed`].filter(Boolean);
  return { text: `${name} was updated${parts.length > 0 ? `: ${parts.join(" and ")}` : ""}.`, names: [name, ...made, ...changed] };
}

/** What the list of exports says: how many, from how many stacks, and which are imported by whom. */
export function exportsSaid(exports: readonly { name: string; stack: string }[], importers: Readonly<Record<string, readonly string[]>>): Said {
  const n = exports.length;
  const stacks = [...new Set(exports.map((e) => e.stack))];
  const head = `${n} ${n === 1 ? "export" : "exports"} from ${stacks.length} ${stacks.length === 1 ? "stack" : "stacks"}.`;
  const used = exports.filter((e) => (importers[e.name] ?? []).length > 0);
  if (used.length === 0) return { text: `${head} Nothing imports ${n === 1 ? "it" : "them"} today.`, names: stacks };
  const lines = used.map((e) => `${nameList(importers[e.name] ?? [])} ${(importers[e.name] ?? []).length === 1 ? "imports" : "import"} ${e.name}`);
  const exporters = [...new Set(used.map((e) => e.stack))];
  const tail = `so ${nameList(exporters)} can't be deleted while ${used.length === 1 && (importers[used[0]?.name ?? ""] ?? []).length === 1 ? "it does" : "they do"}.`;
  const lead = n === 1 ? "It is imported:" : used.length === 1 ? "Only one is imported:" : `${count(used.length)} are imported:`;
  return { text: `${head} ${lead} ${lines.join("; ")}, ${tail}`, names: [...stacks, ...used.map((e) => e.name), ...used.flatMap((e) => importers[e.name] ?? [])] };
}

const dot = (s: string): string => (/[.!?]$/.test(s) ? s : `${s}.`);

/** A resource type as words: `AWS::DynamoDB::Table` is `DynamoDB table`. */
export function typeWords(type: string): string {
  const [, service, thing] = type.split("::");
  return service && thing ? `${service} ${thing.replace(/([a-z])([A-Z])/g, "$1 $2").toLowerCase()}` : type;
}

/** A failure reason without the request token and handler code CloudFormation appends: `already exists.`. */
export const plainReason = (reason: string): string => reason.replace(/\s*\(RequestToken:[^)]*\)\.?\s*$/, "").replace(/^Resource handler returned message: "?/, "").replace(/"$/, "").trim();
