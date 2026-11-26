// Whether the page's tab is hidden. A tail closes its stream while it is (the broker then reads nothing for a tab no one
// looks at) and opens it again from where it was when the tab is seen again.

import { useSyncExternalStore } from "react";

function subscribe(changed: () => void): () => void {
  document.addEventListener("visibilitychange", changed);
  return () => document.removeEventListener("visibilitychange", changed);
}

export function useHidden(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => document.hidden,
    () => false,
  );
}
