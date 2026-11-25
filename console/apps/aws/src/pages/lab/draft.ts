// The input being edited in Send a call, kept while the page is open so that going to Rehearse and back does not lose it, and so that a recipe,
// a saved input or an entry of the history can open the editor with an input of its own. It is for one operation: another operation starts
// from its own example.
import { useSyncExternalStore } from "react";

export type Draft = { target: string; op: string; text: string; /** Where it came from, in words (`From the recipe: …`). */ note: string | null };

let current: Draft | null = null;
const listeners = new Set<() => void>();
const subscribe = (l: () => void) => (listeners.add(l), () => void listeners.delete(l));

/** Opens the editor with `d` for its operation. */
export function setDraft(d: Draft | null): void {
  current = d;
  for (const l of listeners) l();
}

/** The draft, when it is for `target`'s `op`. */
export const draftFor = (d: Draft | null, target: string, op: string): Draft | null => (d && d.target === target && d.op === op ? d : null);

export const useDraft = (): Draft | null => useSyncExternalStore(subscribe, () => current);
export const currentDraft = (): Draft | null => current;

/** An input as the editor shows it. */
export const inputText = (input: unknown): string => JSON.stringify(input, null, 2);
