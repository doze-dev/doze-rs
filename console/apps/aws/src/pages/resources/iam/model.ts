// What the IAM pages work out from what IAM answered: names and aliases checked against IAM's rules, a trust policy read as the
// people it names and built from a choice, key ages, and the words a sentence is made of. Pure: the pages draw them.

/** `a`, `a and b`, `a, b and c`. */
export function list(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** `1 user`, `3 users`. */
export const count = (n: number, one: string, many = `${one}s`): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** `one`, `two`, ... for the sentences that spell a small number out; the figure itself past ten. */
const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
export const spelled = (n: number): string => WORDS[n] ?? String(n);

// ---- what IAM answers ------------------------------------------------------------------------------------------------

export type AccessKey = { UserName: string; AccessKeyId: string; Status: "Active" | "Inactive"; CreateDate: number };
export type KeyUsed = { UserName?: string; AccessKeyLastUsed?: { LastUsedDate?: number; ServiceName?: string; Region?: string } };
export type AttachedPolicy = { PolicyName: string; PolicyArn: string };
export type GroupRef = { GroupName: string; Arn: string; CreateDate: number };

/** The policy a managed policy's ARN names, with where it comes from: `arn:aws:iam::aws:policy/service-role/X` is AWS's. */
export const policyNameOf = (arn: string): string => arn.split("/").pop() ?? arn;
/** The ARN of a customer managed policy a bare name stands for in this account; `null` when `name` is not a bare policy name (an ARN already, or the pseudo-name of an inline policy). A policy's page is at its ARN, and a name typed or bookmarked is taken to it. */
export const policyArnOfName = (account: string, name: string): string | null => (/^[\w+=,.@-]{1,128}$/.test(name) ? `arn:aws:iam::${account}:policy/${name}` : null);
export const isAwsManaged = (arn: string): boolean => arn.startsWith("arn:aws:iam::aws:policy/");
export const managedKind = (arn: string): "AWS managed" | "Customer managed" | "Inline" => (arn.startsWith("inline:") ? "Inline" : isAwsManaged(arn) ? "AWS managed" : "Customer managed");

/** An access key ID cut for a strip: `AKIA...7DEV2K`. */
export const shortKey = (id: string): string => (id.length > 12 ? `${id.slice(0, 4)}...${id.slice(-6)}` : id);

/** Whole days from `fromSec` (epoch seconds) to `nowMs`: `0 days` the day a key was made, as IAM's own report counts them. */
export function daysSince(fromSec: number, nowMs: number): string {
  const n = Math.max(0, Math.floor((nowMs - fromSec * 1000) / 86_400_000));
  return `${n} ${n === 1 ? "day" : "days"}`;
}

/** The access key that is in use, when there are several: an active one first, the newest first. */
export const byUse = (keys: readonly AccessKey[]): AccessKey[] => [...keys].sort((a, b) => Number(b.Status === "Active") - Number(a.Status === "Active") || b.CreateDate - a.CreateDate);

/** `08:09` of an epoch-seconds moment, UTC. */
export const hhmm = (sec: number): string => new Date(sec * 1000).toISOString().slice(11, 16);
/** Whether two moments (epoch seconds, now in ms) fall on the same UTC day. */
export const sameDay = (sec: number, nowMs: number): boolean => new Date(sec * 1000).toISOString().slice(0, 10) === new Date(nowMs).toISOString().slice(0, 10);

/** How long a session may last, as IAM counts it (`MaxSessionDuration`, seconds): `1 hour`, `12 hours`, `45 minutes`. */
export function sessionWords(seconds: number): string {
  if (seconds % 3600 === 0) return count(seconds / 3600, "hour");
  if (seconds % 60 === 0) return count(seconds / 60, "minute");
  return count(seconds, "second");
}

// ---- names and the account alias --------------------------------------------------------------------------------------

/** What IAM allows in a name: letters, digits and `+=,.@_-`. */
const NAME = /^[\w+=,.@-]+$/;
export const NAME_RULES = { user: 64, role: 64, group: 128, policy: 128 } as const;

/** What is wrong with a name typed for a `what` (`user`), in IAM's own limits; `null` when it is fine. */
export function nameProblem(value: string, what: keyof typeof NAME_RULES): string | null {
  const max = NAME_RULES[what];
  if (value === "") return `Enter a ${what} name.`;
  if (value.length > max) return `A ${what} name is at most ${max} characters; this is ${value.length}.`;
  if (!NAME.test(value)) return `A ${what} name has only letters, digits and + = , . @ _ -.`;
  return null;
}

/** An ARN or a path cut after each `:` and `/`, where a line may end: the pieces joined are the text. */
export const breakPoints = (text: string): string[] => text.match(/[^:/]*[:/]?/g)?.filter((s) => s !== "") ?? [];

/** The existing names that differ from `value` only in case: IAM treats them as the same name, and doze-aws refuses the second. */
export const sameIgnoringCase = (value: string, names: readonly string[]): string[] => names.filter((n) => n !== value && n.toLowerCase() === value.toLowerCase());

/** What stops a new `what` called `name` when `taken` holds one of that name (or one that differs only in case: names are not told apart by case); `null` when nothing does. */
export function takenProblem(what: string, name: string, taken: readonly string[]): string | null {
  if (taken.includes(name)) return `A ${what} named ${name} already exists.`;
  const same = sameIgnoringCase(name, taken)[0];
  return same === undefined ? null : `A ${what} named ${same} already exists; names are not told apart by case.`;
}

/** The account alias's rule: 3 to 63 of lowercase letters, digits and hyphens, a letter or digit at each end, no two hyphens in a row. */
export function aliasProblem(value: string): string | null {
  if (value.length < 3 || value.length > 63) return `An alias is 3 to 63 characters; this is ${value.length}.`;
  if (!/^[a-z0-9-]+$/.test(value)) return "An alias has only lowercase letters, digits and hyphens.";
  if (value.startsWith("-") || value.endsWith("-")) return "An alias starts and ends with a letter or digit.";
  if (value.includes("--")) return "An alias cannot hold two hyphens in a row.";
  return null;
}

// ---- trust policies ------------------------------------------------------------------------------------------------------

export type Principal = { kind: "user" | "role" | "root" | "account" | "service" | "federated" | "any" | "other"; /** What to show: the user's name, the service, the ARN. */ label: string; account?: string };

/** A policy document as IAM answers it: its JSON text, that text URL-encoded (as IAM sends a policy of its own), or the JSON it holds. */
export type Doc = string | object;

/** The document as text a person reads: a URL-encoded one decoded, an object written out. */
export function docText(doc: Doc): string {
  if (typeof doc !== "string") return JSON.stringify(doc);
  if (!/^%7B|^%5B/i.test(doc)) return doc;
  try {
    return decodeURIComponent(doc);
  } catch {
    return doc;
  }
}
type Trust = { Statement?: unknown };
const asList = (v: unknown): string[] => (typeof v === "string" ? [v] : Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

const parse = (doc: Doc): Trust | null => {
  try {
    const v: unknown = typeof doc === "string" ? JSON.parse(docText(doc)) : doc;
    return typeof v === "object" && v !== null ? (v as Trust) : null;
  } catch {
    return null;
  }
};

const statements = (doc: Doc): Record<string, unknown>[] => {
  const t = parse(doc);
  const s = t?.Statement;
  return (Array.isArray(s) ? s : s ? [s] : []).filter((x): x is Record<string, unknown> => typeof x === "object" && x !== null);
};

/** One `AWS` principal ARN read: a user or role of an account, an account (`arn:aws:iam::123456789012:root` or the bare ID), or any. */
function awsPrincipal(arn: string): Principal {
  if (arn === "*") return { kind: "any", label: "any principal (*)" };
  if (/^\d{12}$/.test(arn)) return { kind: "account", label: arn, account: arn };
  const m = /^arn:aws[\w-]*:iam::(\d{12}):(root|user\/.+|role\/.+)$/.exec(arn);
  if (!m) return { kind: "other", label: arn };
  const account = m[1]!;
  const rest = m[2]!;
  if (rest === "root") return { kind: "root", label: account, account };
  const name = rest.slice(rest.indexOf("/") + 1);
  return { kind: rest.startsWith("user/") ? "user" : "role", label: name.split("/").pop() ?? name, account };
}

/** The principals a trust policy names, each once, in the order it names them. */
export function trustPrincipals(doc: Doc): Principal[] {
  const out: Principal[] = [];
  const add = (p: Principal) => {
    if (!out.some((x) => x.kind === p.kind && x.label === p.label && x.account === p.account)) out.push(p);
  };
  for (const s of statements(doc)) {
    const p = s.Principal;
    if (p === "*") add({ kind: "any", label: "any principal (*)" });
    else if (typeof p === "object" && p !== null) {
      const o = p as Record<string, unknown>;
      for (const a of asList(o.AWS)) add(awsPrincipal(a));
      for (const sv of asList(o.Service)) add({ kind: "service", label: sv.replace(/\.amazonaws\.com(\.cn)?$/, "") });
      for (const f of asList(o.Federated)) add({ kind: "federated", label: f });
    }
  }
  return out;
}

/** The actions the trust policy allows (`sts:AssumeRole`), each once. */
export function trustActions(doc: Doc): string[] {
  return [...new Set(statements(doc).flatMap((s) => asList(s.Action)))];
}

/** Who may assume a role, as a column says it: `AWS account: 123456789012`, `AWS service: lambda`. Each kind once. */
export function trustedEntities(doc: Doc): string[] {
  const out: string[] = [];
  const add = (t: string) => out.includes(t) || out.push(t);
  for (const p of trustPrincipals(doc)) {
    if (p.kind === "service") add(`AWS service: ${p.label}`);
    else if (p.kind === "federated") add(`Identity provider: ${p.label}`);
    else if (p.kind === "any") add("Everyone (*)");
    else if (p.account) add(`AWS account: ${p.account}`);
    else add(p.label);
  }
  return out;
}

/** What a trust policy says, as a sentence about role `role` in account `account`: `report-job names two principals in this account, ci-bot and dev, and one action, sts:AssumeRole.` */
export function trustSentence(role: string, doc: Doc, account: string): string {
  const ps = trustPrincipals(doc);
  const actions = trustActions(doc);
  if (ps.length === 0) return `${role} has a trust policy that names no principal, so nobody may assume it.`;
  const here = ps.filter((p) => (p.kind === "user" || p.kind === "role") && p.account === account);
  const roots = ps.filter((p) => p.kind === "root" || p.kind === "account");
  const services = ps.filter((p) => p.kind === "service");
  const others = ps.filter((p) => !here.includes(p) && !roots.includes(p) && !services.includes(p));
  const bits: string[] = [];
  if (here.length > 0) bits.push(`${spelled(here.length)} ${here.length === 1 ? "principal" : "principals"} in this account, ${list(here.map((p) => p.label))}`);
  if (roots.length > 0) bits.push(roots.length === 1 && roots[0]!.account === account ? `the whole account ${account}` : `the account${roots.length === 1 ? "" : "s"} ${list(roots.map((p) => p.label))}`);
  if (services.length > 0) bits.push(`the service${services.length === 1 ? "" : "s"} ${list(services.map((p) => p.label))}`);
  if (others.length > 0) bits.push(list(others.map((p) => p.label)));
  const act = actions.length === 0 ? "" : `, and ${spelled(actions.length)} ${actions.length === 1 ? "action" : "actions"}, ${list(actions)}`;
  const head = here.length > 0 ? `names ${bits[0]}` : `names ${bits[0] ?? ""}`;
  const rest = bits.slice(1);
  return `${role} ${head}${rest.length > 0 ? `, ${list(rest)}` : ""}${act}.`;
}

/** Whether a trust policy asks for an external ID: the console cannot switch to such a role. */
export const needsExternalId = (doc: Doc): boolean => /sts:ExternalId/i.test(docText(doc));

export type TrustChoice = {
  type: "service" | "account" | "custom" | "web" | "saml";
  service: string;
  account: "this" | "other";
  other: string;
  external: boolean;
  externalId: string;
  mfa: boolean;
  custom: string;
  /** Web identity and SAML: the ARN of the provider chosen. */
  provider: string;
  /** Web identity: the audience (a client ID of the provider). */
  audience: string;
  /** SAML: the sign-in page is the AWS console's (programmatic and console access), not only the API. */
  console: boolean;
};

/** The services a role is commonly made for, as their principals. */
export const SERVICES: { value: string; label: string }[] = [
  { value: "lambda.amazonaws.com", label: "Lambda" },
  { value: "states.amazonaws.com", label: "Step Functions" },
  { value: "events.amazonaws.com", label: "EventBridge" },
  { value: "scheduler.amazonaws.com", label: "EventBridge Scheduler" },
  { value: "sns.amazonaws.com", label: "SNS" },
  { value: "apigateway.amazonaws.com", label: "API Gateway" },
  { value: "cloudformation.amazonaws.com", label: "CloudFormation" },
];

export const blankTrust: TrustChoice = { type: "account", service: "lambda.amazonaws.com", account: "this", other: "", external: false, externalId: "", mfa: false, custom: "", provider: "", audience: "", console: false };

/** The name a provider's ARN ends with: the host and path of an OpenID Connect provider's URL, or a SAML provider's name. */
export const providerName = (arn: string): string => arn.split(":oidc-provider/")[1] ?? arn.split(":saml-provider/")[1] ?? arn;

/** The condition key a web identity trust policy names the audience by: the provider's URL (without its scheme) and `:aud`. */
export const audienceKey = (providerArn: string): string => `${providerName(providerArn)}:aud`;

/** What a SAML trust policy's `SAML:aud` is: the API alone, or the AWS sign-in page as well. */
export const SAML_AUDIENCE = { programmatic: "https://localhost/saml", console: "https://signin.aws.amazon.com/saml" } as const;

/** The account the AWS account choice trusts: this one's root, or the ID typed. */
export const accountTrusted = (c: TrustChoice, account: string): string => (c.account === "this" ? account : c.other.trim());

/** What is wrong with the choice so far, `null` when it can be built. */
export function trustProblem(c: TrustChoice): string | null {
  if (c.type === "account") {
    if (c.account === "other" && !/^\d{12}$/.test(c.other.trim())) return "An account ID is 12 digits.";
    if (c.external && c.externalId.trim().length < 2) return "An external ID is at least 2 characters.";
  }
  if (c.type === "web") return c.provider === "" ? "Choose an identity provider." : c.audience.trim() === "" ? "Enter the audience." : null;
  if (c.type === "saml") return c.provider === "" ? "Choose a SAML provider." : null;
  if (c.type === "custom") return parse(c.custom) === null ? "The trust policy is not valid JSON." : statements(c.custom).length === 0 ? "The trust policy has no Statement." : null;
  return null;
}

/** The trust policy the choice builds, as the JSON that goes to CreateRole. */
export function trustDocument(c: TrustChoice, account: string): string {
  if (c.type === "custom") return c.custom;
  if (c.type === "web" || c.type === "saml") {
    const web = c.type === "web";
    const statement = {
      Effect: "Allow",
      Principal: { Federated: c.provider || (web ? `arn:aws:iam::${account}:oidc-provider/<provider>` : `arn:aws:iam::${account}:saml-provider/<provider>`) },
      Action: web ? "sts:AssumeRoleWithWebIdentity" : "sts:AssumeRoleWithSAML",
      Condition: { StringEquals: web ? { [audienceKey(c.provider || "<provider>")]: c.audience.trim() || "<audience>" } : { "SAML:aud": c.console ? SAML_AUDIENCE.console : SAML_AUDIENCE.programmatic } },
    };
    return JSON.stringify({ Version: "2012-10-17", Statement: [statement] }, null, 2);
  }
  const principal = c.type === "service" ? { Service: c.service } : { AWS: `arn:aws:iam::${accountTrusted(c, account) || "000000000000"}:root` };
  const statement: Record<string, unknown> = { Effect: "Allow", Principal: principal, Action: "sts:AssumeRole" };
  const condition: Record<string, unknown> = {};
  if (c.type === "account" && c.external) condition.StringEquals = { "sts:ExternalId": c.externalId };
  if (c.type === "account" && c.mfa) condition.Bool = { "aws:MultiFactorAuthPresent": "true" };
  if (Object.keys(condition).length > 0) statement.Condition = condition;
  return JSON.stringify({ Version: "2012-10-17", Statement: [statement] }, null, 2);
}

// ---- policies -----------------------------------------------------------------------------------------------------------

/** One line a policy's statements say, for the review table: the service, the actions and the resource. */
export type Permission = { service: string; actions: string; resource: string };

/** The permissions a policy document grants, a row per statement (and per service its actions name). Allow statements only: a Deny is a
 * limit, not a permission. */
export function permissions(doc: Doc): Permission[] {
  const out: Permission[] = [];
  for (const s of statements(doc)) {
    if (s.Effect !== "Allow") continue;
    const acts = asList(s.Action);
    const res = asList(s.Resource).join(", ") || "*";
    const services = [...new Set(acts.map((a) => a.split(":")[0] ?? a))];
    for (const svc of services.length > 0 ? services : ["*"]) {
      const mine = acts.filter((a) => (a.split(":")[0] ?? a) === svc);
      out.push({ service: svc === "*" ? "All services" : SERVICE_NAMES[svc] ?? svc.toUpperCase(), actions: mine.join(", ") || "*", resource: res });
    }
  }
  return out;
}

const SERVICE_NAMES: Record<string, string> = { s3: "S3", sqs: "SQS", sns: "SNS", lambda: "Lambda", dynamodb: "DynamoDB", logs: "CloudWatch Logs", kms: "KMS", ssm: "Systems Manager", secretsmanager: "Secrets Manager", iam: "IAM", sts: "STS", events: "EventBridge", states: "Step Functions", cloudformation: "CloudFormation", kinesis: "Kinesis", apigateway: "API Gateway", cloudwatch: "CloudWatch" };

/** A starting policy for the editor: one action on one resource, as the board's own example. */
export const starterPolicy = (resource = "arn:aws:sqs:us-east-1:000000000000:pick-lists"): string => JSON.stringify({ Version: "2012-10-17", Statement: [{ Effect: "Allow", Action: ["sqs:SendMessage"], Resource: resource }] }, null, 2);

/** The Resource a statement without one most likely wants, for the refusal's suggestion: the first ARN named in the document's service, else the queue example. */
export function suggestedResource(doc: string, account: string, region: string): string | null {
  const first = statements(doc)[0];
  const action = asList(first?.Action)[0];
  if (!action) return null;
  const [svc] = action.split(":");
  if (svc === "sqs") return `arn:aws:sqs:${region}:${account}:pick-lists`;
  if (svc === "s3") return "arn:aws:s3:::reports/*";
  return `arn:aws:${svc}:${region}:${account}:*`;
}

/** Whether a statement of the document lacks a Resource (and a NotResource): what CreatePolicy answers `MalformedPolicyDocument` to. */
export const lacksResource = (doc: Doc): boolean => statements(doc).some((s) => s.Resource === undefined && s.NotResource === undefined);

/** The AWS managed policies the pickers offer besides the ones IAM lists: names doze-aws makes a document for when they are attached by ARN. */
export const WELL_KNOWN = ["AdministratorAccess", "PowerUserAccess", "ReadOnlyAccess", "AmazonSQSReadOnlyAccess", "AmazonS3ReadOnlyAccess"] as const;
export const awsArn = (name: string): string => `arn:aws:iam::aws:policy/${name}`;
