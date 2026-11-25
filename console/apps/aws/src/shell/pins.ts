// What the rail remembers in this browser: the resources a person pinned, and the ones opened most recently. Both are kept in
// this browser only (localStorage under this console's own keys); doze-aws does not know them. A resource is its address.
import { useSyncExternalStore } from "react";
import { selectedRegion, useSelectedRegion } from "../api/region";
import { PINNED_KEY, VISITED_KEY } from "../nav";
import { subscriptionLabel } from "../pages/resources/sns/sub-model";

export type Place = { service: string; kind: string; name: string };

/** The most recent places the rail lists. */
export const MOST_RECENT = 5;
/** The most places that can be pinned: the rail has room for few. */
export const MOST_PINNED = 8;

/** The resource a path is the page of (`/resources/sqs/queues/pick-lists`), or null for any other path. */
export function placeOf(path: string): Place | null {
  const [, root, service, kind, name, ...rest] = path.split("/");
  if (root !== "resources" || !service || !kind || !name || rest.length > 0) return null;
  try {
    return { service: decodeURIComponent(service), kind: decodeURIComponent(kind), name: decodeURIComponent(name) };
  } catch {
    return null;
  }
}

export const same = (a: Place, b: Place) => a.service === b.service && a.kind === b.kind && a.name === b.name;

/** `places` read from the text kept: only well-formed entries, no repeats. */
export function parse(text: string | null): Place[] {
  if (!text) return [];
  try {
    const v: unknown = JSON.parse(text);
    if (!Array.isArray(v)) return [];
    const out: Place[] = [];
    for (const p of v as Record<string, unknown>[]) {
      if (typeof p?.service === "string" && typeof p.kind === "string" && typeof p.name === "string" && !out.some((o) => same(o, p as Place))) out.push({ service: p.service, kind: p.kind, name: p.name });
    }
    return out;
  } catch {
    return [];
  }
}

/** `list` with `p` put first (once), cut to `most`. */
export const toFront = (list: Place[], p: Place, most: number): Place[] => [p, ...list.filter((x) => !same(x, p))].slice(0, most);

export function togglePin(list: Place[], p: Place): Place[] {
  return list.some((x) => same(x, p)) ? list.filter((x) => !same(x, p)) : [...list, p].slice(-MOST_PINNED);
}

/** The key a list is kept under: each region the console looks at has its own pins and recents (a queue of one region is not a place of
 * another), and the default region keeps the keys it always had. */
export const keyed = (base: string, region: string | null = selectedRegion()): string => (region ? `${base}@${region}` : base);

const listeners = new Set<() => void>();
const read = (key: string) => (typeof localStorage === "undefined" ? null : localStorage.getItem(key));
const write = (key: string, list: Place[]) => {
  try {
    localStorage.setItem(key, JSON.stringify(list));
  } catch {
    /* storage refused (a private window): the rail forgets on reload */
  }
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => (listeners.add(l), window.addEventListener("storage", l), () => (listeners.delete(l), window.removeEventListener("storage", l)));

/** The text kept is the snapshot, so a list is read again only when it changed. */
export const usePinned = (): Place[] => {
  const region = useSelectedRegion();
  return parse(useSyncExternalStore(subscribe, () => read(keyed(PINNED_KEY, region))));
};
export const useVisited = (): Place[] => {
  const region = useSelectedRegion();
  return parse(useSyncExternalStore(subscribe, () => read(keyed(VISITED_KEY, region))));
};

export const pin = (p: Place) => write(keyed(PINNED_KEY), togglePin(parse(read(keyed(PINNED_KEY))), p));
/** Remembers that `p` was opened. */
export const visit = (p: Place) => write(keyed(VISITED_KEY), toFront(parse(read(keyed(VISITED_KEY))), p, MOST_RECENT + MOST_PINNED));

/** Takes `p` out of both lists: a resource that was deleted is not somewhere to go back to. */
export function forget(p: Place) {
  for (const key of [keyed(PINNED_KEY), keyed(VISITED_KEY)]) write(key, parse(read(key)).filter((x) => !same(x, p)));
}

/** Whether `p` is one of `names` of `service`: its name as it is, or as the tail of its ARN or path (a policy is a place by its ARN, and the
 * delete names it by its name). */
export const isGone = (p: Place, service: string, names: readonly string[]): boolean => p.service === service && names.some((n) => n !== "" && (p.name === n || p.name.endsWith(`/${n}`) || p.name.endsWith(`:${n}`)));

/** Takes the places that were deleted out of both lists, for a delete that is a call of its own (a POST: `DeleteAlarms`) and not a DELETE of the
 * resource's address, which `forget` already sees. */
export function forgetGone(service: string, names: readonly string[]) {
  for (const key of [keyed(PINNED_KEY), keyed(VISITED_KEY)]) {
    const kept = parse(read(key));
    const left = kept.filter((p) => !isGone(p, service, names));
    if (left.length !== kept.length) write(key, left);
  }
}

/** Places whose page said, while it is open, that the thing is not there: there is nothing to pin on such a page. */
const absent = new Set<string>();
const keyOf = (p: Place) => JSON.stringify([p.service, p.kind, p.name]);

/** The page of `p` has said it is not there. */
export function markAbsent(p: Place) {
  if (absent.has(keyOf(p))) return;
  absent.add(keyOf(p));
  for (const l of listeners) l();
}

/** Forgets what was said of absent places (the page was left: the thing may exist when it is opened again). */
export function clearAbsent() {
  if (absent.size === 0) return;
  absent.clear();
  for (const l of listeners) l();
}

/** Whether the page of `p` has said it is not there. */
export const useAbsent = (p: Place | null): boolean => useSyncExternalStore(subscribe, () => p !== null && absent.has(keyOf(p)));

/** The resource a request to delete one was for (`/api/v1/resources/sqs/queues/pick-lists?clear=yes`), or null. */
export function deletedBy(url: string): Place | null {
  const at = url.indexOf("/api/v1/resources/");
  if (at < 0) return null;
  const [service, kind, name, ...rest] = (url.slice(at + 18).split("?")[0] ?? "").split("/");
  if (!service || !kind || !name || rest.length > 0) return null;
  try {
    return { service: decodeURIComponent(service), kind: decodeURIComponent(kind), name: decodeURIComponent(name) };
  } catch {
    return null;
  }
}

/** What the rail calls a place: its name, but a customer managed policy is addressed by its ARN, and the rail names the policy. */
export function labelOf(p: Place): string {
  if (p.service === "sns" && p.kind === "subscriptions") return subscriptionLabel(p.name);
  if (p.service === "iam" && p.kind === "policies") {
    const at = p.name.indexOf(":policy/");
    if (p.name.startsWith("arn:") && at >= 0) return p.name.slice(at + 8).split("/").pop() ?? p.name;
  }
  return p.name;
}
