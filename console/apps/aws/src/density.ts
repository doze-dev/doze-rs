// Table density, chosen in Settings and kept in this browser: `compact` lowers the theme's row height (`--row-h`, which every
// DataTable reads when it opens), `comfortable` is the theme's own.
import { useSyncExternalStore } from "react";

export type Density = "comfortable" | "compact";
export const DENSITY_KEY = "doze-aws.density";

const listeners = new Set<() => void>();

function read(): Density {
  try {
    return localStorage.getItem(DENSITY_KEY) === "compact" ? "compact" : "comfortable";
  } catch {
    return "comfortable";
  }
}

/** Puts the choice on <html> (`data-density`), where the stylesheet reads it. */
export function applyDensity(d: Density = read()) {
  if (d === "compact") document.documentElement.dataset.density = "compact";
  else delete document.documentElement.dataset.density;
}

export function setDensity(d: Density) {
  try {
    if (d === "compact") localStorage.setItem(DENSITY_KEY, d);
    else localStorage.removeItem(DENSITY_KEY);
  } catch {
    /* private mode: it lasts this page only */
  }
  applyDensity(d);
  listeners.forEach((l) => l());
}

export function useDensity(): Density {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => (document.documentElement.dataset.density === "compact" ? "compact" : "comfortable"),
  );
}
