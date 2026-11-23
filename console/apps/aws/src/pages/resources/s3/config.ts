// A bucket's settings as the pages read and write them, pure: what S3 answers (Get…) read into rows and words, and what a form
// makes (the Put… input) built from the form. The sentences each tab opens with are here, so they are tested without a server.
import { count, listWords, type Versioning, versioningWords } from "./words";

type Json = Record<string, unknown>;
const obj = (v: unknown): Json => (typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Json) : {});
const list = (v: unknown): Json[] => (Array.isArray(v) ? v.map(obj) : []);
const str = (v: unknown): string | undefined => (typeof v === "string" && v !== "" ? v : undefined);

// --- Block Public Access ----------------------------------------------------------------------------------------------------

export const BPA_KEYS = ["BlockPublicAcls", "IgnorePublicAcls", "BlockPublicPolicy", "RestrictPublicBuckets"] as const;
export type Bpa = Record<(typeof BPA_KEYS)[number], boolean>;
export const BPA_ALL: Bpa = { BlockPublicAcls: true, IgnorePublicAcls: true, BlockPublicPolicy: true, RestrictPublicBuckets: true };

/** The four settings of a `GetPublicAccessBlock` answer; one a bucket never set is off. */
export function bpaOf(answer: unknown): Bpa {
  const c = obj(obj(answer).PublicAccessBlockConfiguration);
  return { BlockPublicAcls: c.BlockPublicAcls === true, IgnorePublicAcls: c.IgnorePublicAcls === true, BlockPublicPolicy: c.BlockPublicPolicy === true, RestrictPublicBuckets: c.RestrictPublicBuckets === true };
}

export const bpaOn = (b: Bpa): number => BPA_KEYS.filter((k) => b[k]).length;

/** The table of the Permissions tab: the master switch (on when all four are), then each setting. */
export function bpaRows(b: Bpa): { setting: string; on: boolean }[] {
  return [{ setting: "Block all public access", on: bpaOn(b) === 4 }, ...BPA_KEYS.map((k) => ({ setting: k, on: b[k] }))];
}

// --- Encryption, ownership --------------------------------------------------------------------------------------------------

export type Sse = "AES256" | "aws:kms" | "aws:kms:dsse";
export const SSE_WORDS: Record<Sse, string> = {
  AES256: "Server-side encryption with Amazon S3 managed keys (SSE-S3)",
  "aws:kms": "Server-side encryption with AWS Key Management Service keys (SSE-KMS)",
  "aws:kms:dsse": "Dual-layer server-side encryption with AWS Key Management Service keys (DSSE-KMS)",
};
export const SSE_SHORT: Record<Sse, string> = { AES256: "SSE-S3", "aws:kms": "SSE-KMS", "aws:kms:dsse": "DSSE-KMS" };
export const isSse = (v: unknown): v is Sse => v === "AES256" || v === "aws:kms" || v === "aws:kms:dsse";

/** The default encryption of a `GetBucketEncryption` answer. */
export function encryptionOf(answer: unknown): { algorithm: Sse; key?: string; bucketKey: boolean } | null {
  const rule = list(obj(obj(answer).ServerSideEncryptionConfiguration).Rules)[0];
  const d = obj(rule?.ApplyServerSideEncryptionByDefault);
  if (!isSse(d.SSEAlgorithm)) return null;
  return { algorithm: d.SSEAlgorithm, key: str(d.KMSMasterKeyID), bucketKey: rule?.BucketKeyEnabled === true };
}

/** The `ServerSideEncryptionConfiguration` of a choice. A key and a Bucket Key belong to KMS only (and DSSE has no Bucket Key). */
export function encryptionConfig(algorithm: Sse, key: string, bucketKey: boolean): Json {
  const d: Json = { SSEAlgorithm: algorithm };
  if (algorithm !== "AES256" && key) d.KMSMasterKeyID = key;
  const rule: Json = { ApplyServerSideEncryptionByDefault: d };
  if (algorithm === "aws:kms") rule.BucketKeyEnabled = bucketKey;
  return { Rules: [rule] };
}

export type Ownership = "BucketOwnerEnforced" | "BucketOwnerPreferred" | "ObjectWriter";
export const OWNERSHIP_WORDS: Record<Ownership, string> = { BucketOwnerEnforced: "Bucket owner enforced", BucketOwnerPreferred: "Bucket owner preferred", ObjectWriter: "Object writer" };
export const isOwnership = (v: unknown): v is Ownership => v === "BucketOwnerEnforced" || v === "BucketOwnerPreferred" || v === "ObjectWriter";
export function ownershipOf(answer: unknown): Ownership | null {
  const v = list(obj(obj(answer).OwnershipControls).Rules)[0]?.ObjectOwnership;
  return isOwnership(v) ? v : null;
}

/** The ACL's grants as rows: who, and what. */
export function aclRows(answer: unknown): { grantee: string; permission: string }[] {
  return list(obj(answer).Grants).map((g) => {
    const who = obj(g.Grantee);
    const uri = str(who.URI);
    const grantee = uri?.endsWith("/AllUsers") ? "Everyone (public access)" : uri?.endsWith("/AuthenticatedUsers") ? "Authenticated users group" : uri ? uri : str(who.DisplayName) === undefined || who.ID === obj(obj(answer).Owner).ID ? "Bucket owner (your AWS account)" : String(who.DisplayName);
    return { grantee, permission: String(g.Permission ?? "") };
  });
}

// --- Policy -----------------------------------------------------------------------------------------------------------------

const ACTIONS: Record<string, string> = { "s3:GetObject": "read objects", "s3:PutObject": "write objects", "s3:DeleteObject": "delete objects", "s3:ListBucket": "list the bucket", "s3:*": "do anything with S3" };

function principalWords(p: unknown): string {
  if (p === "*") return "everyone";
  const aws = obj(p).AWS;
  const arn = Array.isArray(aws) ? str(aws[0]) : str(aws);
  if (arn === "*") return "everyone";
  if (!arn) return "a principal";
  const m = /^arn:aws:iam::(\d+):(root|role\/.+|user\/.+)$/.exec(arn);
  if (!m) return arn;
  return m[2] === "root" ? `account ${m[1]}` : m[2]!.startsWith("role/") ? `the role ${m[2]!.slice(5)}` : `the user ${m[2]!.slice(5)}`;
}

/** The statements of a policy as who and what (`the role lambda-exec`, `read objects`), for the Permissions sentence. */
export function policyClauses(policy: unknown): { who: string; deny: boolean; act: string }[] {
  let doc: unknown = policy;
  if (typeof policy === "string") {
    try {
      doc = JSON.parse(policy);
    } catch {
      return [];
    }
  }
  const st = obj(doc).Statement;
  const statements = Array.isArray(st) ? st.map(obj) : st ? [obj(st)] : [];
  return statements.map((s) => {
    const a = Array.isArray(s.Action) ? s.Action[0] : s.Action;
    return { who: principalWords(s.Principal), deny: s.Effect === "Deny", act: typeof a === "string" ? (ACTIONS[a] ?? a) : "act" };
  });
}

/** `product-images is private: all four Block Public Access settings are on, and its one policy statement lets the role lambda-exec read objects.` */
export function permissionsSentence(name: string, bpa: Bpa, policy: unknown): string {
  const on = bpaOn(bpa);
  const head = on === 4 ? `${name} is private: all four Block Public Access settings are on` : `${name} has ${count(on, "of the four Block Public Access setting")} on`;
  const clauses = policyClauses(policy);
  if (clauses.length === 0) return `${head}, and it has no bucket policy.`;
  const [one] = clauses;
  if (clauses.length === 1 && one) return `${head}, and its one policy statement ${one.deny ? "denies" : "lets"} ${one.who} ${one.act}.`;
  return `${head}, and its bucket policy has ${clauses.length} statements.`;
}

// --- Properties -------------------------------------------------------------------------------------------------------------

export type Rule = { id: string; events: string[]; prefix: string; suffix: string; kind: "Lambda Function" | "SQS Queue" | "SNS Topic"; arn: string; name: string };
const KINDS: [string, string, Rule["kind"]][] = [
  ["LambdaFunctionConfigurations", "LambdaFunctionArn", "Lambda Function"],
  ["QueueConfigurations", "QueueArn", "SQS Queue"],
  ["TopicConfigurations", "TopicArn", "SNS Topic"],
];
const tail = (arn: string) => arn.split(/[:/]/).pop() ?? arn;

/** The event notification rules of a `GetBucketNotificationConfiguration` answer, with the key filter of each read out. */
export function notificationRules(answer: unknown): Rule[] {
  const a = obj(answer);
  return KINDS.flatMap(([member, arnAt, kind]) =>
    list(a[member]).map((r) => {
      const filters = list(obj(obj(r.Filter).Key).FilterRules);
      const rule = (n: string) => str(filters.find((f) => String(f.Name).toLowerCase() === n)?.Value) ?? "";
      const arn = str(r[arnAt]) ?? "";
      return { id: str(r.Id) ?? "", events: Array.isArray(r.Events) ? r.Events.map(String) : [], prefix: rule("prefix"), suffix: rule("suffix"), kind, arn, name: tail(arn) };
    }),
  );
}

/** The configuration with `rule` added to its destination's list; whatever else it holds (EventBridge) is carried. The Put replaces the lot. */
export function withNotification(current: unknown, r: Omit<Rule, "name">): Json {
  const out: Json = { ...obj(current) };
  const [member, arnAt] = KINDS.find((k) => k[2] === r.kind)!.slice(0, 2) as [string, string];
  const filters = [r.prefix && { Name: "prefix", Value: r.prefix }, r.suffix && { Name: "suffix", Value: r.suffix }].filter(Boolean);
  const entry: Json = { ...(r.id ? { Id: r.id } : {}), [arnAt]: r.arn, Events: r.events, ...(filters.length ? { Filter: { Key: { FilterRules: filters } } } : {}) };
  out[member] = [...list(out[member]), entry];
  return out;
}

export type Website = { index: string; error: string; redirectHost: string; redirectProtocol: string } | null;
export function websiteOf(answer: unknown): Website {
  const a = obj(answer);
  const redirect = obj(a.RedirectAllRequestsTo);
  const index = str(obj(a.IndexDocument).Suffix);
  if (!index && !str(redirect.HostName)) return null;
  return { index: index ?? "", error: str(obj(a.ErrorDocument).Key) ?? "", redirectHost: str(redirect.HostName) ?? "", redirectProtocol: str(redirect.Protocol) ?? "" };
}

/** The `WebsiteConfiguration` of the form: hosting an index (and an error) document, or redirecting every request. */
export function websiteConfig(f: { mode: "host" | "redirect"; index: string; error: string; host: string; protocol: string }): Json {
  if (f.mode === "redirect") return { RedirectAllRequestsTo: { HostName: f.host, ...(f.protocol ? { Protocol: f.protocol } : {}) } };
  return { IndexDocument: { Suffix: f.index }, ...(f.error ? { ErrorDocument: { Key: f.error } } : {}) };
}

/** `product-images has 1 event notification rule, versioning on and default encryption SSE-S3. Static website hosting and Object Lock are off.` */
export function propertiesSentence(name: string, p: { versioning: Versioning; notifications: number; encryption: Sse | null; website: boolean; lock: boolean }): string {
  const have = [p.notifications === 0 ? "no event notifications" : count(p.notifications, "event notification rule"), `versioning ${versioningWords(p.versioning)}`, p.encryption ? `default encryption ${SSE_SHORT[p.encryption]}` : "no default encryption"];
  const on = [p.website && "Static website hosting", p.lock && "Object Lock"].filter(Boolean) as string[];
  const off = [!p.website && "Static website hosting", !p.lock && "Object Lock"].filter(Boolean) as string[];
  const tail = [on.length > 0 && `${listWords(on)} ${on.length === 1 ? "is" : "are"} on.`, off.length > 0 && `${listWords(off)} ${off.length === 1 ? "is" : "are"} off.`].filter(Boolean).join(" ");
  return `${name} has ${listWords(have)}. ${tail}`.trim();
}

// --- Management: lifecycle --------------------------------------------------------------------------------------------------

/** One lifecycle rule as the table of the Management tab shows it. */
export type RuleView = { name: string; enabled: boolean; scope: string; actions: string[]; short: string[]; prunesVersions: boolean };

function scopeOf(r: Json): string {
  const f = obj(r.Filter);
  const and = obj(f.And);
  const prefix = str(f.Prefix) ?? str(and.Prefix) ?? str(r.Prefix);
  const tags = and.Tags ? list(and.Tags).length : f.Tag ? 1 : 0;
  const sized = f.ObjectSizeGreaterThan !== undefined || f.ObjectSizeLessThan !== undefined || and.ObjectSizeGreaterThan !== undefined || and.ObjectSizeLessThan !== undefined;
  const bits = [prefix && `Prefix ${prefix}`, tags > 0 && count(tags, "tag"), sized && "object size"].filter(Boolean) as string[];
  return bits.length === 0 ? "Entire bucket" : bits.join(", ");
}

export function lifecycleRules(answer: unknown): RuleView[] {
  return list(obj(answer).Rules).map((r) => {
    const actions: string[] = [];
    const short: string[] = [];
    const exp = obj(r.Expiration);
    if (typeof exp.Days === "number") {
      actions.push(`Expire current versions ${exp.Days} ${exp.Days === 1 ? "day" : "days"} after creation`);
      short.push(`expires objects after ${count(exp.Days, "day")}`);
    }
    if (str(exp.Date)) actions.push(`Expire current versions on ${String(exp.Date).slice(0, 10)}`);
    if (exp.ExpiredObjectDeleteMarker === true) actions.push("Delete expired object delete markers");
    const nc = obj(r.NoncurrentVersionExpiration);
    if (typeof nc.NoncurrentDays === "number") {
      actions.push(`Permanently delete noncurrent versions ${count(nc.NoncurrentDays, "day")} after they become noncurrent${typeof nc.NewerNoncurrentVersions === "number" ? `, keeping the newest ${nc.NewerNoncurrentVersions}` : ""}`);
      short.push(`prunes old versions after ${count(nc.NoncurrentDays, "day")}`);
    }
    const ab = obj(r.AbortIncompleteMultipartUpload);
    if (typeof ab.DaysAfterInitiation === "number") {
      actions.push(`Delete incomplete multipart uploads ${count(ab.DaysAfterInitiation, "day")} after they start`);
      short.push("cleans up unfinished multipart uploads");
    }
    for (const t of list(r.Transitions)) actions.push(`Move current versions to ${String(t.StorageClass)} ${typeof t.Days === "number" ? `${count(t.Days, "day")} after creation` : ""}`.trim());
    for (const t of list(r.NoncurrentVersionTransitions)) actions.push(`Move noncurrent versions to ${String(t.StorageClass)} ${typeof t.NoncurrentDays === "number" ? `${count(t.NoncurrentDays, "day")} after they become noncurrent` : ""}`.trim());
    return { name: str(r.ID) ?? "(no name)", enabled: r.Status === "Enabled", scope: scopeOf(r), actions, short, prunesVersions: typeof nc.NoncurrentDays === "number" };
  });
}

/** `product-images has 1 lifecycle rule: abort-stale-uploads cleans up unfinished multipart uploads. Nothing yet prunes old versions, so each overwrite adds a stored version.` */
export function managementSentence(name: string, rules: RuleView[], versioning: Versioning): string {
  if (rules.length === 0) return `${name} has no lifecycle rules, so objects stay until someone deletes them.`;
  const each = rules.map((r) => `${r.name}${r.short.length > 0 ? ` ${listWords(r.short)}` : " has actions the console does not describe"}`);
  const head = `${name} has ${count(rules.length, "lifecycle rule")}: ${rules.length > 2 ? `${listWords(each.slice(0, 2))} and ${rules.length - 2} more` : listWords(each)}.`;
  return versioning === "Enabled" && !rules.some((r) => r.prunesVersions) ? `${head} Nothing yet prunes old versions, so each overwrite adds a stored version.` : head;
}

export type LifecycleForm = { name: string; allObjects: boolean; prefix: string; tags: { key: string; value: string }[]; min: string; max: string; expireDays: string; noncurrentDays: string; keepNewer: string; abortDays: string; deleteMarkers: boolean };

/** The rule a form makes; the actions the person ticked, the scope as a Filter (empty for the entire bucket). */
export function lifecycleRule(f: LifecycleForm): Json {
  const prefix = f.allObjects ? "" : f.prefix;
  const min = f.min ? Number(f.min) : undefined;
  const max = f.max ? Number(f.max) : undefined;
  // S3 takes one condition as itself and two or more under `And`.
  const tags = f.allObjects ? [] : f.tags.filter((t) => t.key).map((t) => ({ Key: t.key, Value: t.value }));
  const bits: Json = { ...(prefix ? { Prefix: prefix } : {}), ...(min !== undefined ? { ObjectSizeGreaterThan: min } : {}), ...(max !== undefined ? { ObjectSizeLessThan: max } : {}) };
  const conditions = Object.keys(bits).length + tags.length;
  const filter: Json = conditions > 1 ? { And: { ...bits, ...(tags.length > 0 ? { Tags: tags } : {}) } } : tags.length === 1 ? { Tag: tags[0] } : bits;
  const rule: Json = { ID: f.name, Status: "Enabled", Filter: filter };
  if (f.expireDays || f.deleteMarkers) rule.Expiration = f.expireDays ? { Days: Number(f.expireDays) } : { ExpiredObjectDeleteMarker: true };
  if (f.noncurrentDays) rule.NoncurrentVersionExpiration = { NoncurrentDays: Number(f.noncurrentDays), ...(f.keepNewer ? { NewerNoncurrentVersions: Number(f.keepNewer) } : {}) };
  if (f.abortDays) rule.AbortIncompleteMultipartUpload = { DaysAfterInitiation: Number(f.abortDays) };
  return rule;
}

/** The whole configuration with the rule added after those the bucket has (the Put replaces them all). */
export const withRule = (answer: unknown, rule: Json): Json => ({ Rules: [...list(obj(answer).Rules), rule] });

/** What is wrong with a lifecycle form, as the sentence the page shows; `null` when it can be sent. */
export function lifecycleProblem(f: LifecycleForm, existing: string[]): string | null {
  if (!f.name.trim()) return "Name the rule.";
  if (f.name.length > 255) return "A rule's name is at most 255 characters.";
  if (existing.includes(f.name)) return `A rule named ${f.name} is already here: names are unique within the bucket.`;
  const day = (v: string) => v === "" || (/^\d+$/.test(v) && Number(v) >= 1);
  if (![f.expireDays, f.noncurrentDays, f.abortDays, f.keepNewer].every(day)) return "A number of days is a whole number from 1.";
  if (!f.expireDays && !f.noncurrentDays && !f.abortDays && !f.deleteMarkers) return "Choose at least one action.";
  if (f.expireDays && f.deleteMarkers) return "Expire current versions and Delete expired delete markers are separate choices: S3 takes one expiration per rule.";
  if (f.keepNewer && !f.noncurrentDays) return "Newer versions to retain goes with the days after which a version is noncurrent.";
  if (f.allObjects === false && !f.prefix && !f.min && !f.max) return "Give a prefix or a size, or choose the entire bucket.";
  return null;
}
