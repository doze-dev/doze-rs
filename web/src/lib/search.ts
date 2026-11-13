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
  rows?: 1000;
  record?: string;
};
export function gallerySearch(s: Record<string, unknown>): GallerySearch {
  return {
    tab: oneOf(["messages", "partitions", "config"] as const, s.tab),
    density: oneOf(["comfortable", "compact"] as const, s.density),
    rows: s.rows === 1000 || s.rows === "1000" ? 1000 : undefined,
    record: shortText(s.record),
  };
}

// Slice R1b: Create topic, a topic's write overlays, the Config tab.
/** `name`: a name to start from (the Topics list's "Create <filter>"). */
export type CreateSearch = { step?: "config" | "review"; name?: string };
export function createSearch(s: Record<string, unknown>): CreateSearch {
  return { step: oneOf(["config", "review"] as const, s.step), name: shortText(s.name, 249) };
}

export type TopicWriteSearch = { dialog?: "produce" | "delete" | "add-partitions" | "delete-records"; partition?: number };
export function topicWriteSearch(s: Record<string, unknown>): TopicWriteSearch {
  const p = typeof s.partition === "number" ? s.partition : typeof s.partition === "string" && /^\d{1,6}$/.test(s.partition) ? Number(s.partition) : undefined;
  return { dialog: oneOf(["produce", "delete", "add-partitions", "delete-records"] as const, s.dialog), partition: p !== undefined && Number.isInteger(p) && p >= 0 && p < 1_000_000 ? p : undefined };
}

export type ConfigSearch = { q?: string; show?: "changed" | "compaction" | "segments"; edit?: string; reset?: string };
export function configSearch(s: Record<string, unknown>): ConfigSearch {
  return { q: shortText(s.q, 120), show: oneOf(["changed", "compaction", "segments"] as const, s.show), edit: shortText(s.edit, 120), reset: shortText(s.reset, 120) };
}
