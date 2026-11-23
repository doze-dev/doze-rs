// Parameter Store's pages, as plain functions: the list's filter tokens, a parameter as the list reads it, the sentences of the list
// and of a parameter, the history and its labels, and the parameter the form writes.
import { filled, type KeyValueRow } from "@doze/ui/components/app/key-values";
import type { ColumnSpec } from "../../../api/types/ColumnSpec";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { andList, cell } from "../cfg-parts";
import type { FilterProp, FilterToken } from "../filter-bar";
import { timeMs } from "../model";
import { pluralWord } from "@doze/ui/lib/format";

export const FILTERS: FilterProp[] = [
  { value: "Name", label: "Name", options: [{ value: "BeginsWith", label: "begins with" }, { value: "Equals", label: "equals" }, { value: "Contains", label: "contains" }] },
  { value: "Type", label: "Type", choices: [{ value: "String", label: "String" }, { value: "StringList", label: "StringList" }, { value: "SecureString", label: "SecureString" }] },
];

/** The tokens as the address keeps them: `Name:BeginsWith:/larder/staging|Type:Equals:String`, which DescribeParameters' filters are made of. */
export const tokensToQuery = (tokens: readonly FilterToken[]): string => tokens.filter((t) => t.text !== "").map((t) => `${t.prop}:${t.prop === "Type" ? "Equals" : (t.option ?? "BeginsWith")}:${t.text.replaceAll("|", "")}`).join("|");
export function tokensFromQuery(q: string): FilterToken[] {
  return q.split("|").flatMap((t) => {
    const [prop, option, ...rest] = t.split(":");
    const text = rest.join(":");
    const known = FILTERS.find((f) => f.value === prop);
    return known && option && text !== "" ? [{ prop: prop!, ...(known.options ? { option } : {}), text }] : [];
  });
}

export type ParamRow = { name: string; tier: string; type: string; version: string; modified: string | null; description: string; key: string | null };
export function paramRows(rows: readonly ResourceRow[], columns: readonly ColumnSpec[]): ParamRow[] {
  return rows.map((r) => ({ name: r.name, tier: cell(columns, r, "Tier") ?? "Standard", type: cell(columns, r, "Type") ?? "", version: cell(columns, r, "Version") ?? "", modified: cell(columns, r, "Last modified"), description: cell(columns, r, "Description") ?? "", key: cell(columns, r, "KMS key") }));
}

const WHEN = new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric", timeZone: "UTC" });
/** `Nov 7, 2026 at 17:05` (UTC) for a time as the server gave it. */
export function whenWords(v: string | null): string {
  const ms = v === null ? null : timeMs(v);
  if (ms === null) return v ?? "–";
  const d = new Date(ms);
  return `${WHEN.format(d)} at ${String(d.getUTCHours()).padStart(2, "0")}:${String(d.getUTCMinutes()).padStart(2, "0")}`;
}

const count = (n: number, one: string, many = pluralWord(one)) => `${n} ${n === 1 ? one : many}`;

/** `There are 8 parameters in us-east-1: 7 Standard and 1 Advanced. /a is the only SecureString. The newest change is /b, saved as version 2 on Nov 7, 2026 at 17:05.` */
export function listSentence(rows: readonly ParamRow[], region: string): string {
  const n = rows.length;
  const tiers = ["Standard", "Advanced"].map((t) => [t, rows.filter((r) => r.tier === t).length] as const).filter(([, k]) => k > 0).map(([t, k]) => `${k} ${t}`);
  const secure = rows.filter((r) => r.type === "SecureString");
  const head = `There ${n === 1 ? "is" : "are"} ${count(n, "parameter")} in ${region}${tiers.length > 0 ? `: ${andList(tiers)}` : ""}.`;
  const sec = secure.length === 1 ? ` ${secure[0]!.name} is the only SecureString.` : secure.length > 1 ? ` ${secure.length} of them are SecureStrings.` : "";
  const newest = [...rows].filter((r) => r.modified !== null).sort((a, b) => (timeMs(b.modified!) ?? 0) - (timeMs(a.modified!) ?? 0))[0];
  return `${head}${sec}${newest ? ` The newest change is ${newest.name}, saved as version ${newest.version} on ${whenWords(newest.modified)}.` : ""}`;
}

/** `The filter matches 2 of 8 parameters, both Standard String parameters.` */
export function filterSentence(rows: readonly ParamRow[], total: number | null): string {
  const n = rows.length;
  if (n === 0) return `Nothing matches${total !== null ? ` among the ${total} parameters` : ""}.`;
  const kinds = [...new Set(rows.map((r) => `${r.tier} ${r.type}`))];
  const same = kinds.length === 1 ? `, ${n === 1 ? "a" : "all"} ${kinds[0]} parameter${n === 1 ? "" : "s"}` : "";
  return `The filter matches ${n}${total !== null ? ` of ${total} parameters` : ` parameter${n === 1 ? "" : "s"}`}${same}.`;
}

// ---- the form ----

export type Tier = "Standard" | "Advanced";
export type ParamForm = { name: string; description: string; tier: Tier; type: "String" | "StringList" | "SecureString"; key: string | null; value: string; tags: KeyValueRow[] };
export const blankParam = (): ParamForm => ({ name: "", description: "", tier: "Standard", type: "String", key: null, value: "", tags: [] });

/** What a name may be: levels of letters, digits and `. - _` between slashes, up to 15, not beginning with aws or ssm. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Enter a name.";
  const path = name.startsWith("/") ? name.slice(1) : name;
  const levels = path.split("/");
  if (levels.length > 15) return "A name may have up to 15 levels.";
  if (!levels.every((l) => /^[A-Za-z0-9._-]+$/.test(l))) return "Use letters, digits and . - _, with / between levels.";
  if (/^(aws|ssm)/i.test(path)) return "A name cannot begin with aws or ssm: they are reserved.";
  if (name.length > 1011) return "A name is at most 1,011 characters.";
  return null;
}

/** The bytes a tier holds: 4 KB in Standard, 8 KB in Advanced. */
export const LIMITS: Record<Tier, number> = { Standard: 4096, Advanced: 8192 };
export function valueProblem(value: string, tier: Tier): string | null {
  if (value === "") return "Enter a value.";
  if (/\{\{\s*(ssm:)?[^}]*\}\}/.test(value)) return "A value cannot contain {{ }} or {{ssm:name}}.";
  const n = new TextEncoder().encode(value).length;
  return n > LIMITS[tier] ? `A ${tier} parameter holds at most ${LIMITS[tier].toLocaleString("en-US")} characters; this one is ${n.toLocaleString("en-US")}.${tier === "Standard" ? " Choose Advanced for a larger value." : ""}` : null;
}

type Input = Record<string, unknown>;
/** The PutParameter a create makes. Overwrite is not sent, so a name that exists is refused. */
export function putInput(f: ParamForm, o: { overwrite?: boolean; editing?: boolean } = {}): Input {
  const tags = filled(f.tags);
  return {
    Name: f.name,
    ...(f.description ? { Description: f.description } : {}),
    // An edit leaves the type, the key and the data type as they are (PutParameter with Overwrite keeps them, and cannot change the type).
    ...(o.editing ? {} : { Type: f.type }),
    ...(!o.editing && f.type === "SecureString" && f.key ? { KeyId: f.key } : {}),
    Value: f.value,
    Tier: f.tier,
    ...(o.editing ? {} : { DataType: "text" }),
    ...(o.overwrite ? { Overwrite: true } : {}),
    ...(!o.editing && tags.length > 0 ? { Tags: tags.map((t) => ({ Key: t.key, Value: t.value })) } : {}),
  };
}

// ---- history and labels ----

export type Past = { version: number; modified: number | null; description: string; labels: string[]; type: string };
export function pasts(raw: readonly { Version?: number; LastModifiedDate?: number; Description?: string; Labels?: string[]; Type?: string }[]): Past[] {
  return raw.map((p) => ({ version: p.Version ?? 0, modified: p.LastModifiedDate ? p.LastModifiedDate * 1000 : null, description: p.Description ?? "", labels: p.Labels ?? [], type: p.Type ?? "" })).sort((a, b) => b.version - a.version);
}

/** What a label may be: 1 to 100 of letters, digits and `. - _`, not beginning with a digit, aws or ssm. */
export function labelProblem(label: string): string | null {
  if (label === "") return "Enter a label.";
  if (label.length > 100) return "A label is at most 100 characters.";
  if (!/^[A-Za-z0-9._-]+$/.test(label)) return "A label may hold letters, digits and . - _ only.";
  if (/^[0-9]/.test(label)) return "A label cannot begin with a number.";
  if (/^(aws|ssm)/i.test(label)) return "A label cannot begin with aws or ssm.";
  return null;
}

/** Where a label sits now: the version that has it, other than `version`. */
export const labelElsewhere = (ps: readonly Past[], label: string, version: number): number | null => ps.find((p) => p.version !== version && p.labels.includes(label))?.version ?? null;

/** `Saving moves current to version 3, because a label can sit on only one version of a parameter.` */
export function moveWords(ps: readonly Past[], added: readonly string[], version: number): string | null {
  const moves = added.map((l) => [l, labelElsewhere(ps, l, version)] as const).filter(([, v]) => v !== null);
  if (moves.length === 0) return null;
  return `${andList(moves.map(([l, v]) => `${l} is on version ${v} now`))}. Saving moves ${moves.length === 1 ? "it" : "them"} to version ${version}, because a label can sit on only one version of a parameter.`;
}

/** The last words of a parameter's page, said once: its type, tier, version and when it was saved. */
export function versionLine(version: string, modified: string | null): string {
  return `Version ${version}, last modified ${whenWords(modified)} UTC.`;
}

/** What the Value field's teaching line says: a save makes a version and moves nothing, and the labels it leaves where they are
 * (`The label current stays on version 2: ...`, ssm__parameter-edit). */
export function savingWords(ps: readonly Past[]): { lead: string; rest: string | null } {
  const lead = "Saving always creates a new version; the old one stays in History.";
  const held = ps.flatMap((p) => p.labels.map((l) => ({ label: l, version: p.version }))).sort((a, b) => a.version - b.version || a.label.localeCompare(b.label));
  if (held.length === 0) return { lead, rest: null };
  const tail = "a label names one version until you move it.";
  if (held.length === 1) return { lead, rest: `The label ${held[0]!.label} stays on version ${held[0]!.version}: ${tail}` };
  return { lead, rest: `The labels ${andList(held.map((h) => `${h.label} (version ${h.version})`))} stay where they are: ${tail}` };
}
