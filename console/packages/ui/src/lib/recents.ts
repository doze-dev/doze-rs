// The palette's memory: the last five places, newest first, per browser. `kind` is the app's word for what the place is
// (doze-kafka: topic, group, page); the key is the app's too, `doze:recents` unless it names another.
export type Recent = { kind: string; label: string; to: string };

export const RECENTS_KEY = "doze:recents";
export const RECENTS_MOST = 5;

export function pushRecent(list: Recent[], item: Recent): Recent[] {
  return [item, ...list.filter((r) => r.to !== item.to)].slice(0, RECENTS_MOST);
}

/** What was stored, as far as it can be trusted: only entries of the right shape whose address is a path of this console
 * (it starts with `/`, and not `//`, which a browser reads as another site). `kinds`: the words the app knows, if it wants
 * others dropped (an entry from an older build). */
export function parseRecents(raw: string | null, kinds?: readonly string[]): Recent[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v
      .filter((r): r is Recent => {
        if (typeof r !== "object" || r === null) return false;
        const { kind, label, to } = r as Partial<Recent>;
        return typeof kind === "string" && (!kinds || kinds.includes(kind)) && typeof label === "string" && typeof to === "string" && to.startsWith("/") && !to.startsWith("//");
      })
      .map(({ kind, label, to }) => ({ kind, label, to }))
      .slice(0, RECENTS_MOST);
  } catch {
    return [];
  }
}

export function loadRecents(key = RECENTS_KEY, kinds?: readonly string[]): Recent[] {
  try {
    return parseRecents(localStorage.getItem(key), kinds);
  } catch {
    return [];
  }
}

export function saveRecent(item: Recent, key = RECENTS_KEY) {
  try {
    localStorage.setItem(key, JSON.stringify(pushRecent(loadRecents(key), item)));
  } catch {
    // private mode
  }
}
