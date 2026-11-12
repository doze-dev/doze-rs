// Typed search params without a schema library in the main bundle (zod stays in the lazily loaded gallery): each parser
// keeps a value only when it is one of the allowed ones, so a hand-edited address can never put an unknown state on screen.

export function oneOf<const T extends readonly string[]>(allowed: T, raw: unknown): T[number] | undefined {
  return typeof raw === "string" && (allowed as readonly string[]).includes(raw) ? (raw as T[number]) : undefined;
}

export function shortText(raw: unknown, most = 64): string | undefined {
  return typeof raw === "string" && raw.length > 0 && raw.length <= most ? raw : undefined;
}

export type ConnectSearch = { as?: "properties" | "environment" };
export function connectSearch(s: Record<string, unknown>): ConnectSearch {
  return { as: oneOf(["properties", "environment"] as const, s.as) };
}

export type GallerySearch = {
  tab?: "messages" | "partitions" | "config";
  density?: "comfortable" | "compact";
  rows?: "20" | "1000";
  record?: string;
};
export function gallerySearch(s: Record<string, unknown>): GallerySearch {
  return {
    tab: oneOf(["messages", "partitions", "config"] as const, s.tab),
    density: oneOf(["comfortable", "compact"] as const, s.density),
    rows: oneOf(["20", "1000"] as const, typeof s.rows === "number" ? String(s.rows) : s.rows),
    record: shortText(s.record),
  };
}
