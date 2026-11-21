// The create-archive form: the rules of its fields and CreateArchive's input.
import { patternProblem } from "./rule-model";

export type ArchiveForm = { name: string; description: string; bus: string; indefinite: boolean; days: string; filter: boolean; pattern: string };
export const emptyArchive = (bus: string): ArchiveForm => ({ name: "", description: "", bus, indefinite: true, days: "30", filter: false, pattern: '{\n  "source": ["larder.orders"]\n}' });

/** An archive's name: 1 to 48 letters, digits, dot, hyphen and underscore. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Name the archive.";
  if (name.length > 48) return "An archive name is at most 48 characters.";
  return /^[.\-_A-Za-z0-9]+$/.test(name) ? null : "An archive name takes letters, digits, dot, hyphen and underscore only.";
}

/** Retention is a whole number of days, 1 or more; none (indefinite) is sent as no `RetentionDays`. */
export function daysProblem(f: Pick<ArchiveForm, "indefinite" | "days">): string | null {
  if (f.indefinite) return null;
  const n = Number(f.days);
  return Number.isInteger(n) && n >= 1 ? null : "Retention is a whole number of days, 1 or more.";
}

export function archiveProblem(f: ArchiveForm, taken: readonly string[]): string | null {
  return nameProblem(f.name) ?? (taken.includes(f.name) ? `${f.name} is already an archive.` : null) ?? daysProblem(f) ?? (f.filter ? patternProblem(f.pattern) : null);
}

/** CreateArchive's input, with only what was set. */
export function createArchiveInput(f: ArchiveForm, busArn: string): Record<string, unknown> {
  let pattern = f.pattern;
  try {
    pattern = JSON.stringify(JSON.parse(f.pattern));
  } catch {
    /* said on the field */
  }
  return {
    ArchiveName: f.name,
    EventSourceArn: busArn,
    ...(f.description ? { Description: f.description } : {}),
    ...(!f.indefinite ? { RetentionDays: Number(f.days) } : {}),
    ...(f.filter ? { EventPattern: pattern } : {}),
  };
}
