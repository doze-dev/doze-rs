import { pluralWord } from "../../lib/format";
// A table's selection, as pure functions over a set of row ids (no React).

export type AllState = "none" | "some" | "all";

export const toggle = (selected: ReadonlySet<string>, id: string): Set<string> => {
  const next = new Set(selected);
  if (!next.delete(id)) next.add(id);
  return next;
};

/** The header checkbox: all of `ids` when not all are chosen, none when all are. */
export const toggleAll = (selected: ReadonlySet<string>, ids: readonly string[]): Set<string> => (allState(selected, ids) === "all" ? new Set() : new Set(ids));

export function allState(selected: ReadonlySet<string>, ids: readonly string[]): AllState {
  if (ids.length === 0) return "none";
  const n = ids.reduce((a, id) => a + (selected.has(id) ? 1 : 0), 0);
  return n === 0 ? "none" : n === ids.length ? "all" : "some";
}

/** What stays chosen when the rows change (a row that is gone is no longer chosen). */
export const prune = (selected: ReadonlySet<string>, ids: readonly string[]): Set<string> => {
  const live = new Set(ids);
  return new Set([...selected].filter((id) => live.has(id)));
};

/** `3 selected`; with the noun, `3 alarms selected` and `1 alarm selected`. */
export const selectedWords = (n: number, noun?: string) => `${n.toLocaleString("en-US")} ${noun ? `${n === 1 ? noun : pluralWord(noun)} ` : ""}selected`;
