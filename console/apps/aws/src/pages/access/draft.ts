// "Make this pass": the least policy that allows what IAM refused, drafted from IAM's own rows (the exact IAM action and the ARN
// it was asked about), one statement per resource. And who the policy would go on, read from the refused principal's ARN.
import type { Refusal } from "../../api/types/Refusal";

type Statement = { Sid: string; Effect: "Allow"; Action: string[]; Resource: string };

/** A statement id from a resource (letters and digits only, as IAM takes). */
function sid(resource: string, i: number): string {
  if (resource === "*") return "DozeDraftedAnyResource";
  const tail = resource.split(/[:/]/).filter(Boolean).pop() ?? "";
  const clean = tail.replace(/[^A-Za-z0-9]/g, "");
  return `DozeDrafted${clean.slice(0, 40) || "Resource"}${i}`;
}

/** The policy document allowing every refused action on the resource it was refused on, pretty JSON; null for no rows. A
 * refusal by an explicit deny is left out: an allow cannot undo it, and the draft says nothing about it. */
export function draftPolicy(rows: Refusal[]): string | null {
  const by = new Map<string, Set<string>>();
  for (const r of rows.filter((r) => r.decision !== "explicitDeny")) {
    const set = by.get(r.resource) ?? new Set<string>();
    set.add(r.action);
    by.set(r.resource, set);
  }
  if (by.size === 0) return null;
  const statements: Statement[] = [...by.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([resource, actions], i) => ({ Sid: sid(resource, i + 1), Effect: "Allow", Action: [...actions].sort(), Resource: resource }));
  return JSON.stringify({ Version: "2012-10-17", Statement: statements }, null, 2);
}

/** Whether any of the rows is an explicit deny (which no allow can lift). */
export const anyExplicit = (rows: Refusal[]) => rows.some((r) => r.decision === "explicitDeny");

/** The IAM user or role a principal's ARN names: `arn:aws:iam::1:user/path/name` is user `name`; an assumed-role session
 * `arn:aws:sts::1:assumed-role/role/session` is role `role`. */
export function holderOf(arn: string): { kind: "user" | "role"; name: string } | null {
  const user = /^arn:aws:iam::\d+:user\/(?:.*\/)?([^/]+)$/.exec(arn);
  if (user?.[1]) return { kind: "user", name: user[1] };
  const role = /^arn:aws:sts::\d+:assumed-role\/([^/]+)\/[^/]+$/.exec(arn);
  if (role?.[1]) return { kind: "role", name: role[1] };
  return null;
}

/** A policy name for a draft (`doze-sqs-ListQueues`), as IAM takes it. */
export function policyName(actions: string[]): string {
  const first = actions[0]?.replace(":", "-") ?? "access";
  return `doze-${first}${actions.length > 1 ? "-and-more" : ""}`.replace(/[^\w+=,.@-]/g, "").slice(0, 128);
}
