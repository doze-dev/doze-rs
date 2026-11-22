// KMS's pages, as plain functions: a key as the list reads it (its aliases, its state in words), the sentences of the list and of a
// key, what uses a key, and the key policy as the sections the console writes (administrators, deletion, users) and back.
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { andList, cell } from "../cfg-parts";
import { timeMs } from "../model";

export const aliasLabel = (name: string): string => (name.startsWith("alias/") ? name.slice(6) : name);

export type KeyRow = { id: string; aliases: string[]; state: string; stateWords: string; kind: "Symmetric" | "Asymmetric" | "HMAC" | "Key"; spec: string; usage: string; managedByAws: boolean; created: string | null; deletion: string | null; description: string };

/** The kind of key a spec is (the server words it the same, for a key's badge). */
export function kindOf(spec: string): KeyRow["kind"] {
  if (spec.startsWith("HMAC")) return "HMAC";
  if (spec.startsWith("SYMMETRIC")) return "Symmetric";
  if (spec.startsWith("RSA") || spec.startsWith("ECC")) return "Asymmetric";
  return "Key";
}

/** A state as people say it: `PendingDeletion` as `Pending deletion`. */
export function stateWords(state: string): string {
  return state.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/^(.)(.*)$/, (_, a: string, b: string) => a + b.toLowerCase());
}

/** The keys of a list page with the aliases each has: `aliases` are the rows of the aliases list (name `alias/x`, the key in its `Key` cell). */
export function keyRows(rows: readonly ResourceRow[], columns: readonly ColumnSpec[], aliases: readonly { name: string; key: string }[]): KeyRow[] {
  return rows.map((r) => {
    const state = cell(columns, r, "State") ?? "";
    const spec = cell(columns, r, "Spec") ?? "";
    return {
      id: r.name,
      aliases: aliases.filter((a) => a.key === r.name).map((a) => aliasLabel(a.name)).sort(),
      state,
      stateWords: stateWords(state),
      kind: kindOf(spec),
      spec,
      usage: cell(columns, r, "Usage") ?? "",
      managedByAws: cell(columns, r, "Managed by") === "AWS",
      created: cell(columns, r, "Created"),
      deletion: cell(columns, r, "Deletion date"),
      description: cell(columns, r, "Description") ?? "",
    };
  });
}

/** What the key is called in the list: its first alias, `(+1 more)` for the others; none is a dash. */
export const aliasWords = (k: Pick<KeyRow, "aliases">): string => (k.aliases.length === 0 ? "–" : k.aliases.length === 1 ? k.aliases[0]! : `${k.aliases[0]} (+${k.aliases.length - 1} more)`);
/** The name the key's page and sentences use: its first alias, else its id. */
export const keyName = (k: Pick<KeyRow, "aliases" | "id">): string => k.aliases[0] ?? k.id;

/** `5 customer managed keys: 3 enabled, 1 disabled and 1 pending deletion.` */
export function countSentence(keys: readonly KeyRow[], noun: string): string {
  const by = (state: string) => keys.filter((k) => k.state === state).length;
  const parts = [["enabled", by("Enabled")], ["disabled", by("Disabled")], ["pending deletion", by("PendingDeletion")], ["pending import", by("PendingImport")]].filter(([, n]) => (n as number) > 0).map(([w, n]) => `${n} ${w}`);
  return `${keys.length} ${noun}${keys.length === 1 ? "" : "s"}${parts.length > 0 ? `: ${andList(parts)}` : ""}.`;
}

/** `6 AWS managed keys, all Enabled.` (or the count by state), and what their aliases look like. */
export function awsSentence(keys: readonly KeyRow[]): string {
  const n = keys.length;
  const all = n > 0 && keys.every((k) => k.state === "Enabled");
  const head = all ? `${n} AWS managed key${n === 1 ? "" : "s"}, all Enabled.` : countSentence(keys, "AWS managed key");
  return `${head} Each alias has the form aws/<service-name>.`;
}

const DATE = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
/** `Nov 15, 2026 at 08:14` (UTC) for a time as the server gave it. */
export function whenWords(value: string): string {
  const ms = timeMs(value);
  if (ms === null) return value;
  const d = new Date(ms);
  return `${DATE.format(d)} at ${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
}

/** What each key pending deletion says: when it goes, and that deletion can be cancelled. */
export function pendingSentence(keys: readonly KeyRow[]): string {
  return keys.filter((k) => k.state === "PendingDeletion" && k.deletion).map((k) => `${keyName(k)} will be deleted on ${whenWords(k.deletion!)} unless you cancel.`).join(" ");
}

export type Use = { service: "sqs" | "secretsmanager" | "ssm"; kind: "queues" | "secrets" | "parameters"; name: string; noun: string };

/** The names a key answers to in other services' settings: its id, its ARN and each alias as `alias/x`. */
export const refsOf = (key: Pick<KeyRow, "id" | "aliases">, arn?: string): Set<string> => new Set([key.id, ...(arn ? [arn] : []), ...key.aliases.map((a) => `alias/${a}`)]);

/** What of `uses` is encrypted with a key: a use names its key by id, ARN, alias or alias ARN. */
export function usedBy(uses: readonly (Use & { key: string | null })[], refs: ReadonlySet<string>): Use[] {
  return uses.filter((u) => u.key !== null && (refs.has(u.key) || [...refs].some((r) => u.key!.endsWith(`:${r}`) || u.key!.endsWith(`/${r}`)))).map(({ service, kind, name, noun }) => ({ service, kind, name, noun }));
}

/** `alias/larder-app encrypts the queue pick-lists.` */
export function encryptsSentence(label: string, uses: readonly Use[]): string | null {
  if (uses.length === 0) return null;
  const nouns: Record<Use["kind"], string> = { queues: "queue", secrets: "secret", parameters: "parameter" };
  const shown = uses.slice(0, 3).map((u) => `${nouns[u.kind]} ${u.name}`);
  return `${label} encrypts the ${andList(shown)}${uses.length > 3 ? ` and ${uses.length - 3} more` : ""}.`;
}

// ---- the key policy as the console writes it ----

export const SIDS = { account: "Enable IAM User Permissions", admins: "Allow access for Key Administrators", use: "Allow use of the key", grants: "Allow attachment of persistent resources" } as const;

export const ADMIN_ACTIONS = ["kms:Create*", "kms:Describe*", "kms:Enable*", "kms:List*", "kms:Put*", "kms:Update*", "kms:Revoke*", "kms:Disable*", "kms:Get*", "kms:Delete*", "kms:TagResource", "kms:UntagResource", "kms:ScheduleKeyDeletion", "kms:CancelKeyDeletion", "kms:RotateKeyOnDemand"];
const DELETION = ["kms:ScheduleKeyDeletion", "kms:CancelKeyDeletion"];

/** What a key may be used for, by usage: the actions the console grants a key user. */
export const USE_ACTIONS: Record<string, string[]> = {
  ENCRYPT_DECRYPT: ["kms:Encrypt", "kms:Decrypt", "kms:ReEncrypt*", "kms:GenerateDataKey*", "kms:DescribeKey"],
  SIGN_VERIFY: ["kms:Sign", "kms:Verify", "kms:GetPublicKey", "kms:DescribeKey"],
  GENERATE_VERIFY_MAC: ["kms:GenerateMac", "kms:VerifyMac", "kms:DescribeKey"],
  KEY_AGREEMENT: ["kms:DeriveSharedSecret", "kms:GetPublicKey", "kms:DescribeKey"],
};

export type Statement = { Sid?: string; Effect?: string; Principal?: unknown; Action?: unknown; Resource?: unknown; Condition?: unknown };
export type Policy = { Version?: string; Id?: string; Statement?: Statement[] };

const one = <T>(v: T[]): T | T[] => (v.length === 1 ? v[0]! : v);

/** The key policy the console writes for a key: the account (so IAM policies can grant access), the administrators, the users, and
 * the grants AWS services make for them. A statement is written only when someone is named for it. */
export function buildPolicy(o: { account: string; admins: readonly string[]; canDelete: boolean; users: readonly string[]; usage: string }): Policy {
  const statements: Statement[] = [{ Sid: SIDS.account, Effect: "Allow", Principal: { AWS: `arn:aws:iam::${o.account}:root` }, Action: "kms:*", Resource: "*" }];
  if (o.admins.length > 0) statements.push({ Sid: SIDS.admins, Effect: "Allow", Principal: { AWS: one([...o.admins]) }, Action: o.canDelete ? ADMIN_ACTIONS : ADMIN_ACTIONS.filter((a) => !DELETION.includes(a)), Resource: "*" });
  if (o.users.length > 0) {
    statements.push({ Sid: SIDS.use, Effect: "Allow", Principal: { AWS: one([...o.users]) }, Action: USE_ACTIONS[o.usage] ?? USE_ACTIONS.ENCRYPT_DECRYPT, Resource: "*" });
    if (o.usage === "ENCRYPT_DECRYPT") statements.push({ Sid: SIDS.grants, Effect: "Allow", Principal: { AWS: one([...o.users]) }, Action: ["kms:CreateGrant", "kms:ListGrants", "kms:RevokeGrant"], Resource: "*", Condition: { Bool: { "kms:GrantIsForAWSResource": true } } });
  }
  return { Version: "2012-10-17", Id: "key-consolepolicy-3", Statement: statements };
}

export function parsePolicy(text: string): Policy | null {
  try {
    const v = JSON.parse(text) as unknown;
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Policy) : null;
  } catch {
    return null;
  }
}

const list = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : typeof v === "string" ? [v] : []);
const principals = (s: Statement | undefined): string[] => {
  const p = s?.Principal;
  if (typeof p === "string") return [p];
  return typeof p === "object" && p !== null ? list((p as { AWS?: unknown }).AWS) : [];
};

/** The name of a principal as the console shows it: `role/larder-admin` becomes `larder-admin` and `role`; `root` is the account. */
export function principalWords(arn: string): { name: string; kind: "role" | "user" | "account" | "other" } {
  const m = /^arn:aws:iam::(\d+):(root|user|role)(?:\/(.+))?$/.exec(arn);
  if (!m) return { name: arn, kind: "other" };
  if (m[2] === "root") return { name: m[1]!, kind: "account" };
  return { name: (m[3] ?? "").split("/").pop() ?? "", kind: m[2] as "user" | "role" };
}

export type PolicySections = { admins: string[]; canDelete: boolean; users: string[] };

/** The policy as the sections the console's own wording shows, when it is one the console wrote (its statements carry the Sids the
 * console gives them); `null` for any other policy, which is shown as the JSON it is. */
export function policySections(text: string): PolicySections | null {
  const p = parsePolicy(text);
  const statements = p?.Statement;
  if (!Array.isArray(statements)) return null;
  const by = (sid: string) => statements.find((s) => s.Sid === sid);
  const known = new Set<string>(Object.values(SIDS));
  if (!by(SIDS.account) || statements.some((s) => !s.Sid || !known.has(s.Sid))) return null;
  const admin = by(SIDS.admins);
  return { admins: principals(admin), canDelete: admin ? DELETION.every((d) => list(admin.Action).includes(d)) : false, users: principals(by(SIDS.use)) };
}

/** `2 statements`; the Sids when there are few. */
export function statementWords(text: string): string {
  const s = parsePolicy(text)?.Statement;
  const n = Array.isArray(s) ? s.length : 0;
  return `${n} statement${n === 1 ? "" : "s"}`;
}

/** What the list says under its table about the keys chosen and what Key actions do to them. */
export function actionWords(chosen: readonly KeyRow[]): string {
  const names = chosen.map(keyName);
  const states = [...new Set(chosen.map((k) => k.stateWords))];
  const lead = chosen.length === 0 ? "" : `${andList(names)} ${chosen.length === 1 ? "is" : "are"} selected and ${andList(states)}. `;
  return `${lead}Key actions are Enable, Disable, Schedule key deletion and Cancel key deletion. Cancel key deletion is refused for a key that is not Pending deletion (KMSInvalidStateException, "is not pending deletion"); Enable, Disable and Schedule key deletion are refused for one that is. Disabling is the safer step before deleting: a disabled key can be enabled again, a deleted one cannot.`;
}

export type KeyAction = "enable" | "disable" | "schedule" | "cancel";

/** Why an action cannot be made on the keys chosen (the service would refuse it); `null` when it can. */
export function whyNot(action: KeyAction, chosen: readonly KeyRow[]): string | null {
  if (chosen.length === 0) return "Select a key first.";
  const pending = chosen.filter((k) => k.state === "PendingDeletion");
  if (action === "cancel") return pending.length === chosen.length ? null : "Cancel key deletion is for a key that is pending deletion.";
  return pending.length === 0 ? null : `A key that is pending deletion cannot be ${action === "enable" ? "enabled" : action === "disable" ? "disabled" : "scheduled again"}.`;
}

const NUMBERS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
/** `two`, `no`, `11`: a small count in words. */
export const countWord = (n: number): string => NUMBERS[n] ?? String(n);
const cap = (s: string) => `${s[0]!.toUpperCase()}${s.slice(1)}`;

/** `Two aliases and three tags.` */
export function countsSentence(aliases: number, tags: number | null): string {
  const a = `${countWord(aliases)} ${aliases === 1 ? "alias" : "aliases"}`;
  if (tags === null) return `${cap(a)}.`;
  return `${cap(a)} and ${countWord(tags)} ${tags === 1 ? "tag" : "tags"}.`;
}

export type RotationFacts = { enabled: boolean; days: number | null; last: { type: string; at: string } | null; count: number };

const TYPES: Record<string, string> = { ON_DEMAND: "on demand", AUTOMATIC: "automatically" };

/** What a key's page says of itself: its state, when its material was last rotated, and whether it rotates by itself. Only a symmetric key rotates. */
export function keySentence(label: string, key: Pick<KeyRow, "state" | "stateWords" | "deletion" | "kind">, rotation: RotationFacts | null): string {
  if (key.state === "PendingDeletion") return `${label} is ${key.stateWords}${key.deletion ? ` and will be deleted on ${whenWords(key.deletion)} unless you cancel` : ""}.`;
  const head = `${label} is ${key.stateWords}.`;
  if (key.kind !== "Symmetric" || !rotation) return head;
  const last = rotation.last ? `Its key material was rotated ${TYPES[rotation.last.type] ?? "by rotation"} on ${DATE.format(new Date(timeMs(rotation.last.at) ?? 0))}, and` : "Its key material has not been rotated, and";
  return `${head} ${last} automatic rotation is ${rotation.enabled ? `on, every ${rotation.days ?? 365} days` : "off"}.`;
}
