// What the Lab keeps in this browser: a list under a key of its own for each region the console looks at (a call sent to eu-west-1 is
// not in us-east-1's history). doze-aws does not know these lists; a private window forgets them on reload.
import { useMemo, useSyncExternalStore } from "react";
import { selectedRegion, useSelectedRegion } from "../../api/region";

export const SENT_KEY = "doze-aws.lab.sent";
export const SAVED_KEY = "doze-aws.lab.saved";

/** The key a list is kept under for `region` (`null`: the server's default region). */
export const keyed = (base: string, region: string | null): string => `${base}.${region ?? "default"}`;

const listeners = new Set<() => void>();
const read = (key: string): string | null => {
  try {
    return globalThis.localStorage?.getItem(key) ?? null;
  } catch {
    return null;
  }
};

/** `text` read as a list: only the entries `parse` accepts, so a hand-edited or older value shows what is whole of it. */
export function parseList<T>(text: string | null, parse: (v: unknown) => T | null): T[] {
  if (!text) return [];
  try {
    const v: unknown = JSON.parse(text);
    return Array.isArray(v) ? v.map(parse).filter((x): x is T => x !== null) : [];
  } catch {
    return [];
  }
}

/** The list kept under `base` for the region now chosen. */
export const readKept = <T>(base: string, parse: (v: unknown) => T | null, region: string | null = selectedRegion()): T[] => parseList(read(keyed(base, region)), parse);

/** Keeps `list` under `base` for the region now chosen (`region` for a write made after the choice moved on). */
export function writeKept<T>(base: string, list: T[], region: string | null = selectedRegion()): void {
  try {
    globalThis.localStorage?.setItem(keyed(base, region), JSON.stringify(list));
  } catch {
    /* storage refused (a private window, or full): the list is forgotten on reload */
  }
  for (const l of listeners) l();
}

const subscribe = (l: () => void) => (listeners.add(l), window.addEventListener("storage", l), () => (listeners.delete(l), window.removeEventListener("storage", l)));

/** The list kept for the region chosen, drawn again when it changes (here or in another tab of the browser). The text kept is the snapshot,
 * so the list is read again only when it changed. */
export function useKept<T>(base: string, parse: (v: unknown) => T | null): T[] {
  const region = useSelectedRegion();
  const text = useSyncExternalStore(subscribe, () => read(keyed(base, region)));
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `parse` is a module function of the caller
  return useMemo(() => parseList(text, parse), [text]);
}
