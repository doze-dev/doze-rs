// The sentences of the IAM pages that the page builds from what it has read: the line under a list's title and under a resource's.
// Plain text; the page sets the names and figures in bold. Pure.
import { count, list, spelled } from "./model";

export type UserFacts = { name: string; active: number; usedToday: boolean };

/** `3 users: dev and ci-bot each hold 1 active access key and both used it today; temp-auditor has no keys.` */
export function usersSentence(users: readonly UserFacts[]): string {
  if (users.length === 0) return "There are no users in this account.";
  const head = `${count(users.length, "user")}`;
  const keyed = users.filter((u) => u.active > 0);
  const bare = users.filter((u) => u.active === 0);
  const clauses: string[] = [];
  if (keyed.length > 0) {
    const sameCount = keyed.every((u) => u.active === keyed[0]!.active);
    const holds = sameCount ? `${list(keyed.map((u) => u.name))} ${keyed.length === 1 ? "holds" : "each hold"} ${count(keyed[0]!.active, "active access key")}` : keyed.map((u) => `${u.name} holds ${count(u.active, "active access key")}`).join(", ");
    const today = keyed.filter((u) => u.usedToday);
    // A user holding two keys: "it" would be read as one key, so the words say a key.
    const multi = keyed.some((u) => u.active > 1);
    const used = multi
      ? today.length === 0 ? "none used a key today" : today.length === keyed.length ? (keyed.length === 1 ? "a key was used today" : keyed.length === 2 ? "both used a key today" : "all used a key today") : `${list(today.map((u) => u.name))} used a key today`
      : today.length === 0 ? "none used one today" : today.length === keyed.length ? (keyed.length === 1 ? "it was used today" : keyed.length === 2 ? "both used it today" : "all used it today") : `${list(today.map((u) => u.name))} used ${today.length === 1 ? "it" : "theirs"} today`;
    clauses.push(`${holds} and ${used}`);
  }
  if (bare.length > 0) clauses.push(`${list(bare.map((u) => u.name))} ${bare.length === 1 ? "has" : "have"} no access keys`);
  return `${head}: ${clauses.join("; ")}.`;
}

export type GroupFacts = { name: string; members: string[]; policies: string[] };

/** `1 group: developers has 1 member, dev, and one attached policy, PowerUserAccess.` */
export function groupsSentence(groups: readonly GroupFacts[]): string {
  if (groups.length === 0) return "There are no user groups in this account.";
  const one = (g: GroupFacts): string => {
    const m = g.members.length === 0 ? "no members" : `${count(g.members.length, "member")}${g.members.length <= 3 ? `, ${list(g.members)},` : ""}`;
    const p = g.policies.length === 0 ? "no attached policy" : `${spelled(g.policies.length)} attached ${g.policies.length === 1 ? "policy" : "policies"}${g.policies.length <= 3 ? `, ${list(g.policies)}` : ""}`;
    return `${g.name} has ${m} and ${p}`;
  };
  // The groups that hold something first: they are what the sentence is about.
  const busy = [...groups].sort((a, b) => b.members.length + b.policies.length - (a.members.length + a.policies.length) || a.name.localeCompare(b.name));
  const shown = busy.slice(0, 3).map(one);
  const more = groups.length > 3 ? `; ${groups.length - 3} more` : "";
  return `${count(groups.length, "group")}: ${shown.join("; ")}${more}.`;
}

/** What a service principal is, said as the roles' sentence says it. */
const SERVICE_WORDS: Record<string, string> = { lambda: "Lambda functions", states: "Step Functions", events: "EventBridge", scheduler: "EventBridge Scheduler", sns: "SNS", apigateway: "API Gateway", cloudformation: "CloudFormation" };

export type RoleFacts = { name: string; trusted: string[]; assumed: number; inLastHour: number; assumedBy: string[] };

/** `3 roles. report-job was assumed 7 times in the last hour...`: what the record says of who took a session, and for which service the role is. */
export function rolesSentence(roles: readonly RoleFacts[]): string {
  if (roles.length === 0) return "There are no roles in this account.";
  // The roles with sessions first: they are what the sentence is about.
  const busy = [...roles].sort((a, b) => b.assumed - a.assumed);
  const clauses = busy.slice(0, 5).map((r) => {
    if (r.assumed > 0) return `${r.name} was assumed ${count(r.assumed, "time")}${r.inLastHour === r.assumed ? " in the last hour" : " in the Live record"}${r.assumedBy.length === 1 ? `, each time with ${r.assumedBy[0]}'s key` : r.assumedBy.length > 1 ? `, with the keys of ${list(r.assumedBy)}` : ""}`;
    const svc = r.trusted.filter((t) => t.startsWith("AWS service: ")).map((t) => SERVICE_WORDS[t.slice("AWS service: ".length)] ?? t.slice("AWS service: ".length));
    if (svc.length > 0) return `${r.name} is for ${list(svc)}`;
    return `${r.name} was not assumed in the Live record`;
  });
  return `${count(roles.length, "role")}. ${clauses.join("; ")}${roles.length > 5 ? `; ${roles.length - 5} more` : ""}.`;
}

export type PolicyFacts = { name: string; attached: { kind: "user" | "role" | "group"; name: string }[] };

/** `2 customer managed policies are attached: report-job-readonly to the role report-job, larder-queues-rw to the user ci-bot.` */
export function policiesSentence(policies: readonly PolicyFacts[], scope: "Local" | "AWS"): string {
  const noun = scope === "Local" ? "customer managed polic" : "AWS managed polic";
  const plural = (n: number) => `${n} ${noun}${n === 1 ? "y" : "ies"}`;
  if (policies.length === 0) return scope === "Local" ? "There are no customer managed policies in this account." : "No AWS managed policy is attached to anything yet, so IAM lists none.";
  const bound = policies.filter((p) => p.attached.length > 0);
  const free = policies.filter((p) => p.attached.length === 0);
  const to = (p: PolicyFacts) => `${p.name} to ${list(p.attached.slice(0, 3).map((a) => `the ${a.kind} ${a.name}`))}${p.attached.length > 3 ? ` and ${p.attached.length - 3} more` : ""}`;
  const parts: string[] = [];
  if (bound.length > 0) parts.push(`${plural(bound.length)} ${bound.length === 1 ? "is" : "are"} attached: ${bound.slice(0, 4).map(to).join(", ")}${bound.length > 4 ? ` and ${bound.length - 4} more` : ""}`);
  if (free.length > 0) parts.push(`${bound.length === 0 ? plural(free.length) : free.length === 1 ? "1 is" : `${free.length} are`} not attached to anything${free.length <= 3 ? `: ${list(free.map((p) => p.name))}` : ""}`);
  return `${parts.join("; ")}.${scope === "Local" ? " AWS managed policies are not counted here." : ""}`;
}

/** What an AWS managed policy allows, in a few words, for the policies doze-aws makes a document for (crates/aws-iam/src/awsmanaged.rs). */
export const POLICY_WORDS: Record<string, string> = {
  AdministratorAccess: "everything",
  PowerUserAccess: "every service except IAM, Organizations and Account",
  ReadOnlyAccess: "the read actions of every service",
};

type KeyFact = { id: string; usedAt: number | null; service: string | null; region: string | null };
export type UserSentence = { name: string; groups: string[]; viaGroups: string[]; direct: string[]; key: KeyFact | null; /** The other active keys, when the user holds more than one: each is named, so none is read as the only one. */ others?: KeyFact[] };

/** `dev belongs to 1 group and gets PowerUserAccess through it. Its key AKIA… was last used at 08:09 with sqs in us-east-1.` */
export function userSentence(u: UserSentence): string {
  const g = u.groups.length === 0 ? "no group" : count(u.groups.length, "group");
  const via = [...new Set(u.viaGroups)];
  const parts: string[] = [];
  if (via.length > 0) parts.push(`gets ${list(via)} through ${u.groups.length === 1 ? "it" : "them"}`);
  if (u.direct.length > 0) parts.push(`has ${list(u.direct)} attached directly`);
  const head = parts.length === 0 ? `${u.name} belongs to ${g} and has no policy, so it is allowed to do nothing.` : `${u.name} belongs to ${g} and ${parts.join(" and ")}.`;
  const where = (k: KeyFact): string => (k.usedAt === null ? "has never been used" : `was last used at ${hhmmOf(k.usedAt)}${k.service ? ` with ${k.service}${k.region ? ` in ${k.region}` : ""}` : ""}`);
  const keys = u.key ? [u.key, ...(u.others ?? [])] : [];
  const tail = keys.length === 0 ? "" : keys.length === 1 ? ` Its key ${keys[0]!.id} ${where(keys[0]!)}.` : ` Of its keys, ${keys.map((k) => `${k.id} ${where(k)}`).join(" and ")}.`;
  return `${head}${tail}`;
}

const hhmmOf = (sec: number): string => new Date(sec * 1000).toISOString().slice(11, 16);

/** `developers has 1 member and 1 attached policy. Everyone in it can do what PowerUserAccess allows: every service except IAM, Organizations and Account.` */
export function groupSentence(name: string, members: readonly string[], policies: readonly string[]): string {
  const m = members.length === 0 ? "no members" : count(members.length, "member");
  const p = policies.length === 0 ? "no attached policy" : count(policies.length, "attached policy", "attached policies");
  if (policies.length === 0) return `${name} has ${m} and ${p}, so its members get nothing from it.`;
  const words = policies.length === 1 ? POLICY_WORDS[policies[0]!] : undefined;
  return `${name} has ${m} and ${p}. ${members.length === 0 ? "Anyone added" : "Everyone in it"} can do what ${list(policies)} ${policies.length === 1 ? "allows" : "allow"}${words ? `: ${words}` : ""}.`;
}

/** `dev-intern inherits PowerUserAccess through developers: every service except IAM, Organizations and Account.` The words follow a policy only
 * when one policy is inherited and doze-aws knows what it allows. */
export function inheritsSentence(user: string, policies: readonly string[], groups: readonly string[]): string {
  const once = [...new Set(policies)];
  const words = once.length === 1 ? POLICY_WORDS[once[0]!] : undefined;
  return `${user} inherits ${list(once)} through ${list(groups)}${words ? `: ${words}` : ""}.`;
}

export type RoleSentence = { name: string; assumed: number; inLastHour: number; by: string[]; policies: string[]; only: { action: string; resource: string } | null };

/** `report-job was assumed 7 times in the last hour, all with ci-bot's key. Its only permission is s3:GetObject on reports/*, through report-job-readonly.` */
export function roleSentence(r: RoleSentence): string {
  const when = r.inLastHour === r.assumed ? " in the last hour" : " in the Live record";
  const who = r.by.length === 0 ? "" : r.by.length === 1 ? `, all with ${r.by[0]}'s key` : `, with the keys of ${list(r.by)}`;
  const head = r.assumed === 0 ? `${r.name} has not been assumed in the Live record` : `${r.name} was assumed ${count(r.assumed, "time")}${when}${who}`;
  const perm = r.policies.length === 0 ? "It has no permissions policy, so a session of it is allowed to do nothing."
    : r.only && r.policies.length === 1 ? `Its only permission is ${r.only.action} on ${r.only.resource}, through ${r.policies[0]}.`
    : `It holds ${list(r.policies)}.`;
  return `${head}. ${perm}`;
}

/** `report-job-readonly is attached to 1 role, report-job. Its default version is v1; the policy has not been edited since it was created.` */
export function policySentence(name: string, entities: { users: string[]; roles: string[]; groups: string[] }, version: string, edited: boolean, aws: boolean): string {
  const parts = [
    entities.roles.length > 0 && `${count(entities.roles.length, "role")}, ${list(entities.roles)}`,
    entities.users.length > 0 && `${count(entities.users.length, "user")}, ${list(entities.users)}`,
    entities.groups.length > 0 && `${count(entities.groups.length, "group")}, ${list(entities.groups)}`,
  ].filter(Boolean) as string[];
  const to = parts.length === 0 ? `${name} is not attached to anything.` : `${name} is attached to ${list(parts)}.`;
  return `${to} Its default version is ${version}; ${aws ? "doze-aws makes its document from its name." : edited ? "the policy has been edited since it was created." : "the policy has not been edited since it was created."}`;
}

/** `dev has 1 access key, and it is active.` (the Security credentials tab). */
export function keysSentence(name: string, keys: readonly { Status: string }[]): string {
  if (keys.length === 0) return `${name} has no access keys.`;
  const active = keys.filter((k) => k.Status === "Active").length;
  const inactive = keys.length - active;
  const state = keys.length === 1 ? (active === 1 ? "it is active" : "it is inactive") : `${active === 0 ? "none" : count(active, "is", "are")} ${active === 0 ? "is active" : "active"}${inactive > 0 && active > 0 ? ` and ${inactive} inactive` : ""}`;
  return `${name} has ${count(keys.length, "access key")}, and ${state}.`;
}
