// The palette's memory: the last five places, newest first, per browser (`doze:recents`).
export type Recent = { kind: "topic" | "group" | "page"; label: string; to: string };

export const RECENTS_KEY = "doze:recents";
export const RECENTS_MOST = 5;

export function pushRecent(list: Recent[], item: Recent): Recent[] {
  return [item, ...list.filter((r) => r.to !== item.to)].slice(0, RECENTS_MOST);
}

export function parseRecents(raw: string | null): Recent[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    if (!Array.isArray(v)) return [];
    return v
      .filter(
        (r): r is Recent =>
          typeof r === "object" && r !== null && ["topic", "group", "page"].includes((r as Recent).kind) && typeof (r as Recent).label === "string" && typeof (r as Recent).to === "string" && (r as Recent).to.startsWith("/"),
      )
      .slice(0, RECENTS_MOST);
  } catch {
    return [];
  }
}

export function loadRecents(): Recent[] {
  try {
    return parseRecents(localStorage.getItem(RECENTS_KEY));
  } catch {
    return [];
  }
}

export function saveRecent(item: Recent) {
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(pushRecent(loadRecents(), item)));
  } catch {
    // private mode
  }
}
