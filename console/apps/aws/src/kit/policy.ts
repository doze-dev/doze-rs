// An IAM policy document, read as a person needs to see it: its statements in words, and what is plainly wrong. A check of
// the shape only (IAM validates the rest when the policy is put): it never says a policy is right, only what is wrong with it.

export type Statement = { index: number; sid: string | null; effect: string; actions: string[]; notActions: boolean; resources: string[]; notResources: boolean; principals: string[]; condition: boolean };
export type PolicyKind = "identity" | "resource";
export type PolicyCheck = { ok: boolean; /** What is wrong, one sentence each. */ problems: string[]; statements: Statement[] };

const list = (v: unknown): string[] => (typeof v === "string" ? [v] : Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
const principalList = (v: unknown): string[] => {
  if (v === "*") return ["*"];
  if (typeof v !== "object" || v === null) return [];
  return Object.entries(v as Record<string, unknown>).flatMap(([kind, ids]) => list(ids).map((i) => (i === "*" ? "*" : `${kind}: ${i}`)));
};

/** The checks. A resource policy (a queue's access policy, a bucket policy) names a Principal in each statement; an identity
 * policy (attached to a user, role or group) does not. IAM counts a managed policy's size without whitespace: 6,144 at most. */
export function checkPolicy(text: string, kind: PolicyKind = "identity", maxSize?: number): PolicyCheck {
  const problems: string[] = [];
  if (text.trim() === "") return { ok: false, problems: ["The policy is empty."], statements: [] };
  let doc: unknown;
  try {
    doc = JSON.parse(text);
  } catch (e) {
    return { ok: false, problems: [`It is not JSON: ${(e as Error).message}`], statements: [] };
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) return { ok: false, problems: ["A policy is a JSON object with a Version and a Statement."], statements: [] };
  const d = doc as Record<string, unknown>;
  if (d.Version !== "2012-10-17" && d.Version !== "2008-10-17") problems.push(d.Version === undefined ? 'It has no Version; use "2012-10-17".' : `Version ${JSON.stringify(d.Version)} is not one IAM knows; use "2012-10-17".`);
  const raw = d.Statement === undefined ? [] : Array.isArray(d.Statement) ? d.Statement : [d.Statement];
  if (raw.length === 0) problems.push("It has no Statement.");
  const sids = new Set<string>();
  const statements: Statement[] = [];
  raw.forEach((s, i) => {
    const at = `Statement ${i + 1}`;
    if (typeof s !== "object" || s === null || Array.isArray(s)) {
      problems.push(`${at} is not an object.`);
      return;
    }
    const st = s as Record<string, unknown>;
    if (st.Effect !== "Allow" && st.Effect !== "Deny") problems.push(`${at} needs an Effect of Allow or Deny.`);
    if (st.Action === undefined && st.NotAction === undefined) problems.push(`${at} needs an Action.`);
    if (kind === "identity" && st.Resource === undefined && st.NotResource === undefined) problems.push(`${at} needs a Resource.`);
    if (kind === "resource" && st.Principal === undefined && st.NotPrincipal === undefined) problems.push(`${at} needs a Principal: a resource policy says who it is for.`);
    if (kind === "identity" && st.Principal !== undefined) problems.push(`${at} has a Principal, which a policy attached to a user, group or role cannot have.`);
    const sid = typeof st.Sid === "string" ? st.Sid : null;
    if (sid !== null) {
      if (sids.has(sid)) problems.push(`Sid ${JSON.stringify(sid)} is used twice.`);
      sids.add(sid);
    }
    statements.push({ index: i, sid, effect: typeof st.Effect === "string" ? st.Effect : "?", actions: list(st.Action ?? st.NotAction), notActions: st.NotAction !== undefined, resources: list(st.Resource ?? st.NotResource), notResources: st.NotResource !== undefined, principals: principalList(st.Principal ?? st.NotPrincipal), condition: st.Condition !== undefined });
  });
  if (maxSize !== undefined) {
    const size = text.replace(/\s/g, "").length;
    if (size > maxSize) problems.push(`It is ${size.toLocaleString("en-US")} characters without spaces; the most is ${maxSize.toLocaleString("en-US")}.`);
  }
  return { ok: problems.length === 0, problems, statements };
}

/** A statement in a sentence: `Allow sqs:SendMessage on arn:aws:sqs:…` (and, in a resource policy, to whom). */
export function statementWords(s: Statement): string {
  const acts = s.actions.length === 0 ? "nothing" : s.actions.length > 3 ? `${s.actions.slice(0, 3).join(", ")} and ${s.actions.length - 3} more` : s.actions.join(", ");
  const res = s.resources.length === 0 ? "" : ` ${s.notResources ? "on everything but" : "on"} ${s.resources.length > 2 ? `${s.resources[0]} and ${s.resources.length - 1} more` : s.resources.join(", ")}`;
  const who = s.principals.length === 0 ? "" : ` for ${s.principals.join(", ")}`;
  return `${s.effect} ${s.notActions ? "everything but " : ""}${acts}${res}${who}${s.condition ? ", with a condition" : ""}`;
}
