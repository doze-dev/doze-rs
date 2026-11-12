// The refresh interval of every data page (live-design.md): Off by default, 5 s at least, one preference for all pages,
// remembered per browser in localStorage under `doze:refresh`. A tiny external store so every RefreshControl and every query
// reads the same value and changes together.
import { useSyncExternalStore } from "react";

export const INTERVALS = [
  { value: "off", label: "Off", ms: 0 },
  { value: "5s", label: "5 s", ms: 5_000 },
  { value: "10s", label: "10 s", ms: 10_000 },
  { value: "30s", label: "30 s", ms: 30_000 },
  { value: "1m", label: "1 min", ms: 60_000 },
] as const;

export type IntervalValue = (typeof INTERVALS)[number]["value"];

export const STORAGE_KEY = "doze:refresh";

/** A stored value, or Off for anything that is not one of the choices (a value from an older build, a hand edit). */
export function parseInterval(raw: string | null | undefined): IntervalValue {
  return INTERVALS.find((i) => i.value === raw)?.value ?? "off";
}

export function intervalMs(v: IntervalValue): number {
  return INTERVALS.find((i) => i.value === v)?.ms ?? 0;
}

const listeners = new Set<() => void>();

function read(): IntervalValue {
  try {
    return parseInterval(globalThis.localStorage?.getItem(STORAGE_KEY));
  } catch {
    return "off";
  }
}

let current: IntervalValue = read();

export function setInterval_(v: IntervalValue) {
  current = v;
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, v);
  } catch {
    // private mode: the choice lasts as long as the page
  }
  for (const l of listeners) l();
}

function subscribe(l: () => void) {
  listeners.add(l);
  const onStorage = (e: StorageEvent) => {
    if (e.key === STORAGE_KEY) {
      current = parseInterval(e.newValue);
      l();
    }
  };
  globalThis.addEventListener?.("storage", onStorage);
  return () => {
    listeners.delete(l);
    globalThis.removeEventListener?.("storage", onStorage);
  };
}

export function useRefreshInterval(): [IntervalValue, (v: IntervalValue) => void] {
  const v = useSyncExternalStore(subscribe, () => current, () => "off" as IntervalValue);
  return [v, setInterval_];
}
