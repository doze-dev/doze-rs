// The create-bus form: its fields' rules, the requests it makes (CreateEventBus, then CreateArchive), and what a refusal says.
import { type KeyValueRow, filled } from "@doze/ui/components/app/key-values";

export type BusForm = { name: string; description: string; cmk: boolean; kms: string | null; dlq: string | null; archive: boolean; archiveName: string; archiveDescription: string; tags: KeyValueRow[] };
export const emptyBus: BusForm = { name: "", description: "", cmk: false, kms: null, dlq: null, archive: false, archiveName: "", archiveDescription: "", tags: [] };

/** A custom bus name: 1 to 256 letters, digits, dot, hyphen and underscore; not `default`, and no `/`. */
export function busNameProblem(name: string): string | null {
  if (name === "") return "Name the bus.";
  if (name === "default") return "default is the name of the default bus.";
  if (name.length > 256) return "A bus name is at most 256 characters.";
  if (name.includes("/")) return "A custom bus name can't contain /.";
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : "A bus name takes letters, digits, dot, hyphen and underscore only.";
}

/** An archive's name: 1 to 48 letters, digits, dot, hyphen and underscore. */
export function archiveNameProblem(name: string): string | null {
  if (name === "") return "Name the archive.";
  if (name.length > 48) return "An archive name is at most 48 characters.";
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : "An archive name takes letters, digits, dot, hyphen and underscore only.";
}

export const defaultArchiveName = (bus: string): string => `${bus}-archive`.slice(0, 48);

/** CreateEventBus's input: only what was set (empty sections are left out). */
export function createBusInput(f: BusForm): Record<string, unknown> {
  const tags = filled(f.tags).map((r) => ({ Key: r.key, Value: r.value }));
  return {
    Name: f.name,
    ...(f.description ? { Description: f.description } : {}),
    ...(f.cmk && f.kms ? { KmsKeyIdentifier: f.kms } : {}),
    ...(f.dlq ? { DeadLetterConfig: { Arn: f.dlq } } : {}),
    ...(tags.length > 0 ? { Tags: tags } : {}),
  };
}

/** CreateArchive's input for the archive made with the bus: no retention (indefinite) and no pattern. */
export const createArchiveInput = (f: BusForm, busArn: string): Record<string, unknown> => ({ ArchiveName: f.archiveName, EventSourceArn: busArn, ...(f.archiveDescription ? { Description: f.archiveDescription } : {}) });

/** The collapsed sections' words when the page shows a refusal. */
export function encryptionSummary(f: BusForm): string {
  const key = f.cmk && f.kms ? `Customer managed key ${f.kms}` : "AWS owned key";
  return `${key}${f.dlq ? `, with ${f.dlq.split(":").pop()} as the dead-letter queue` : ""}. Stored, not applied locally.`;
}
export const archiveSummary = (f: BusForm): string => (f.archive ? `Enabled, named ${f.archiveName}.` : "Off.");
export const tagsSummary = (f: BusForm): string => {
  const t = filled(f.tags);
  return t.length === 0 ? "No tags." : `${t.map((r) => (r.value ? `${r.key}=${r.value}` : r.key)).join(", ")}.`;
};
