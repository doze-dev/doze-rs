// The refresh interval of every data page (live-design.md): Off by default, 5 s at least, one preference for all pages,
// remembered per browser in localStorage under `doze:refresh`. A tiny external store so every RefreshControl and every query
// reads the same value and changes together.
import { useEffect, useSyncExternalStore } from "react";

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

// ---- a page whose Snapshot | Live tail switch owns its updates ----
// Live and a topic's Messages have their own switch: the page-wide RefreshControl is not shown there (two ways of updating
// one page would be two concepts), so the interval chosen on other pages must not poll there either. While such a page is
// mounted it holds this claim; every snapshot query reads the interval through `usePollInterval`.

let owners = 0;
const ownerListeners = new Set<() => void>();

function claim(): () => void {
  owners += 1;
  for (const l of ownerListeners) l();
  return () => {
    owners -= 1;
    for (const l of ownerListeners) l();
  };
}

/** Called by a page whose tail switch owns its updates: while it is mounted no snapshot query polls on the interval. */
export function useTailOwnsRefresh() {
  useEffect(() => claim(), []);
}

function useOwned(): boolean {
  return useSyncExternalStore(
    (l) => {
      ownerListeners.add(l);
      return () => ownerListeners.delete(l);
    },
    () => owners > 0,
    () => false,
  );
}

/** The refetch interval of a snapshot query: the person's choice in ms, or false when Off or when a tail owns the page. */
export function usePollInterval(): number | false {
  const [v] = useRefreshInterval();
  const owned = useOwned();
  const ms = intervalMs(v);
  return ms > 0 && !owned ? ms : false;
}
