// The region the console is looking at: one choice for the whole console, kept in this browser. It rides on every request as
// `X-Console-Region`; none chosen means the server's default (`--region`).
import { useSyncExternalStore } from "react";

export const REGION_KEY = "doze-aws.region";
export const REGION_HEADER = "X-Console-Region";

function read(): string | null {
  try {
    return globalThis.localStorage?.getItem(REGION_KEY) || null;
  } catch {
    return null;
  }
}

let selected: string | null = read();
const listeners = new Set<() => void>();

/** The region chosen, or `null` for the default. */
export const selectedRegion = (): string | null => selected;

/** Chooses a region (`null`: the default again). */
export function setSelectedRegion(code: string | null): void {
  if (code === selected) return;
  selected = code;
  try {
    if (code) globalThis.localStorage?.setItem(REGION_KEY, code);
    else globalThis.localStorage?.removeItem(REGION_KEY);
  } catch {
    // private mode: the choice holds while the page is open
  }
  for (const l of listeners) l();
}

/** The region chosen, redrawn when it changes. */
export function useSelectedRegion(): string | null {
  return useSyncExternalStore(
    (on) => {
      listeners.add(on);
      return () => listeners.delete(on);
    },
    selectedRegion,
    () => null,
  );
}

/** The header a request carries for the region chosen (none for the default), for a fetch that does not go through the client. */
export function regionHeader(): Record<string, string> {
  return selected ? { [REGION_HEADER]: selected } : {};
}
