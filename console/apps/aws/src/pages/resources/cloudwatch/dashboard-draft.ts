// A dashboard's body being worked on: the widgets added and removed since it was saved are held here, so they are still there when the
// page of a widget (which is a page of its own) returns to the dashboard. Held in memory for this tab only; Save dashboard writes it.
import { useSyncExternalStore } from "react";

const drafts = new Map<string, string>();
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export const getDraft = (name: string): string | undefined => drafts.get(name);

export function setDraft(name: string, body: string | null): void {
  if (body === null) drafts.delete(name);
  else drafts.set(name, body);
  emit();
}

/** The draft of `name`, and a re-render when it changes. */
export function useDraft(name: string): string | undefined {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => drafts.get(name),
  );
}
