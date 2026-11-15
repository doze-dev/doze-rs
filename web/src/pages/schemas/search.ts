// The typed search params of the Schemas pages: filters, the version on screen and the overlays live in the address, and a
// hand-edited address can only ever put a known value on screen.
import { oneOf, shortText } from "../../lib/search";

export const KINDS = ["AVRO", "JSON", "PROTOBUF"] as const;
export type Kind = (typeof KINDS)[number];
export const KIND_WORD: Record<string, string> = { AVRO: "Avro", JSON: "JSON Schema", PROTOBUF: "Protobuf" };

const flag = (v: unknown): 1 | undefined => (v === 1 || v === "1" ? 1 : undefined);
const whole = (v: unknown): number | undefined => {
  const n = typeof v === "number" ? v : typeof v === "string" && /^\d{1,9}$/.test(v) ? Number(v) : NaN;
  return Number.isSafeInteger(n) && n > 0 ? n : undefined;
};

export type SchemasSearch = { q?: string; type?: Kind[]; deleted?: 1 };
export function schemasSearch(s: Record<string, unknown>): SchemasSearch {
  const raw = Array.isArray(s.type) ? s.type : s.type === undefined ? [] : [s.type];
  const type = KINDS.filter((k) => raw.includes(k));
  return { q: shortText(s.q, 249), type: type.length ? type : undefined, deleted: flag(s.deleted) };
}

export const SUBJECT_DIALOGS = ["compatibility", "delete-version", "delete-subject", "try"] as const;
export type SubjectDialog = (typeof SUBJECT_DIALOGS)[number];
export type SubjectSearch = { version?: number; dialog?: SubjectDialog; stored?: 1 };
export function subjectSearch(s: Record<string, unknown>): SubjectSearch {
  return { version: whole(s.version), dialog: oneOf(SUBJECT_DIALOGS, s.dialog), stored: flag(s.stored) };
}

export type DiffSearch = { from?: number; to?: number; layout?: "unified" };
export function diffSearch(s: Record<string, unknown>): DiffSearch {
  return { from: whole(s.from), to: whole(s.to), layout: oneOf(["unified"] as const, s.layout) };
}

export type NewSchemaSearch = { subject?: string; type?: Kind };
export function newSchemaSearch(s: Record<string, unknown>): NewSchemaSearch {
  return { subject: shortText(s.subject, 1024), type: oneOf(KINDS, s.type) };
}
