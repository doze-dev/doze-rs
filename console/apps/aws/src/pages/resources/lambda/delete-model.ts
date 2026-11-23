// What the delete dialog and the list that follows it say: what goes with the function (its versions and aliases) and what stays
// (its mappings, the topic that still points at it). Pure.
import type { Blockers } from "../../../api/types/Blockers";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { Trigger } from "./model";
import { pluralWord } from "@doze/ui/lib/format";

const count = (n: number, one: string, many = pluralWord(one)) => `${n} ${n === 1 ? one : many}`;
const names = (xs: string[]): string => (xs.length <= 1 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);

/** `Deleting send-email permanently deletes the function, its 3 versions and its 2 aliases (live, staging). A deleted function cannot be recovered.` */
export function deleteWords(name: string, b: Blockers | undefined): string {
  const group = (t: string) => b?.blockers.find((g) => g.title === t)?.items.map((i) => i.name) ?? [];
  const versions = group("Versions");
  const aliases = group("Aliases");
  const goes = [versions.length ? `its ${count(versions.length, "version")}` : null, aliases.length ? `its ${count(aliases.length, "alias", "aliases")} (${aliases.join(", ")})` : null].filter(Boolean) as string[];
  return `Deleting ${name} permanently deletes the function${goes.length ? `, ${names(goes)}` : ""}. A deleted function cannot be recovered.`;
}

/** What a delete leaves, as the list it lands on says it: the mappings that still exist and the topics that still point. Carried in the address (`sel`). */
export type Left = { mappings: { uuid: string; queue: string }[]; topics: string[] };

export function leftBehind(d: ResourceDetail, triggers: readonly Trigger[]): string {
  const left: Left = {
    mappings: triggers.filter((t) => t.how === "mapping").map((t) => ({ uuid: t.id ?? "", queue: t.name })),
    topics: triggers.filter((t) => t.kind === "SNS topic").map((t) => t.name),
  };
  void d;
  return JSON.stringify(left);
}

export function readLeft(sel: string | undefined): Left {
  try {
    const v = JSON.parse(sel ?? "") as Partial<Left>;
    return { mappings: Array.isArray(v.mappings) ? v.mappings : [], topics: Array.isArray(v.topics) ? v.topics : [] };
  } catch {
    return { mappings: [], topics: [] };
  }
}

/** The follow-up sentences of the list after a delete. */
export function leftWords(name: string, left: Left): string[] {
  const out: string[] = [];
  if (left.topics.length) out.push(`The ${names(left.topics)} ${left.topics.length === 1 ? "topic still points" : "topics still point"} at the deleted function; remove ${left.topics.length === 1 ? "that subscription" : "those subscriptions"} in SNS.`);
  if (left.mappings.length === 1) out.push(`Event source mappings are separate resources: the mapping for ${left.mappings[0]?.queue} (${left.mappings[0]?.uuid}) still exists. Delete it with DeleteEventSourceMapping.`);
  else if (left.mappings.length > 1) out.push(`Event source mappings are separate resources: the mappings for ${names(left.mappings.map((m) => m.queue))} still exist. Delete each with DeleteEventSourceMapping.`);
  void name;
  return out;
}

/** What a delete left behind, kept for the list the person lands on (by function name): it is a one-time result, so it stays out of the address. */
const LEFT = new Map<string, string>();
export const rememberLeft = (name: string, left: string): void => void LEFT.set(name, left);
export const recalledLeft = (name: string): string | undefined => LEFT.get(name);
