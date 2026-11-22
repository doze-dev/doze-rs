// "Add to policy": the policy the caller already has gets the statements it lacks, instead of another policy being made. Pure: a
// policy document is read, asked whether a statement of it covers an action on a resource, and given the statements that are
// missing (an S3 object ARN widens to the bucket's objects, `arn:aws:s3:::receipts/*`, as the board does).
import type { Refusal } from "../../api/types/Refusal";

export type Statement = { Sid?: string; Effect?: string; Action?: string | string[]; NotAction?: string | string[]; Resource?: string | string[]; NotResource?: string | string[]; Condition?: unknown; Principal?: unknown };

const list = (v: string | string[] | undefined): string[] => (v === undefined ? [] : Array.isArray(v) ? v : [v]);

/** The statements of a policy document's JSON text; none for text that is not one. */
export function statementsOf(document: string | null): Statement[] {
  if (!document) return [];
  try {
    const v = JSON.parse(document) as { Statement?: Statement | Statement[] };
    return v.Statement === undefined ? [] : Array.isArray(v.Statement) ? v.Statement : [v.Statement];
  } catch {
    return [];
  }
}

/** Whether `pattern` (IAM's `*` and `?` wildcards, case-insensitive for actions) matches `value`. */
export function wild(pattern: string, value: string, fold = false): boolean {
  const re = new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")}$`, fold ? "i" : "");
  return re.test(value);
}

/** Whether an Allow statement covers `action` on `resource` (a statement with a Condition, a NotAction or a NotResource is left
 * alone: this does not evaluate those). */
export function allows(st: Statement, action: string, resource: string): boolean {
  if (st.Effect !== "Allow" || st.Condition !== undefined || st.NotAction !== undefined || st.NotResource !== undefined) return false;
  return list(st.Action).some((a) => wild(a, action, true)) && list(st.Resource).some((r) => wild(r, resource));
}

/** How many statements of `doc` cover the action on the resource. */
export const covering = (doc: string | null, action: string, resource: string): number => statementsOf(doc).filter((s) => allows(s, action, resource)).length;

/** `arn:aws:s3:::receipts/r-8841.pdf` as `arn:aws:s3:::receipts/*`: the bucket's objects; any other ARN as it is. */
export function widen(resource: string): string {
  const m = /^(arn:aws:s3:::[^/]+)\/.+$/.exec(resource);
  return m ? `${m[1]}/*` : resource;
}

const sidOf = (action: string, resource: string): string => `Allow${(action.split(":").pop() ?? "").replace(/[^A-Za-z0-9]/g, "")}${(resource.split(/[:/]/).filter((p) => p && p !== "*").pop() ?? "").replace(/[^A-Za-z0-9]/g, "")}`.slice(0, 64);

/** `document` with a statement for each refusal in `rows` it does not already cover, (actions on one resource together). `added`
 * says what was put in, one line each (`s3:GetObject on arn:aws:s3:::receipts/*`). An explicit deny is left out: an allow cannot
 * lift it. */
export function addTo(document: string, rows: readonly Refusal[]): { document: string; added: string[] } {
  const parsed = JSON.parse(document) as { Version?: string; Statement?: Statement | Statement[] };
  const have = statementsOf(document);
  const by = new Map<string, Set<string>>();
  for (const r of rows.filter((r) => r.decision !== "explicitDeny")) {
    const resource = widen(r.resource);
    if (have.some((s) => allows(s, r.action, resource) && allows(s, r.action, r.resource))) continue;
    by.set(resource, (by.get(resource) ?? new Set()).add(r.action));
  }
  const added: string[] = [];
  const fresh: Statement[] = [...by].map(([resource, actions]) => {
    const a = [...actions].sort();
    added.push(`${a.join(", ")} on ${resource}`);
    return { Sid: sidOf(a[0] ?? "", resource), Effect: "Allow", Action: a.length === 1 ? a[0] : a, Resource: resource };
  });
  // A Sid already in use is made apart.
  const used = new Set(have.map((s) => s.Sid));
  for (const s of fresh) for (let n = 2; s.Sid && used.has(s.Sid); n++) s.Sid = `${s.Sid?.replace(/\d+$/, "")}${n}`;
  return { document: JSON.stringify({ ...parsed, Version: parsed.Version ?? "2012-10-17", Statement: [...have, ...fresh] }, null, 2), added };
}
