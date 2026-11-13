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

// Slice R4: Settings, Access, Lab. Each overlay is a search param (Back closes it; a deep link opens it).
export const DYNAMIC_KEYS = ["log.segment.bytes", "log.index.interval.bytes", "message.max.bytes", "log.retention.bytes", "log.retention.ms"] as const;
export type SettingsSearch = { edit?: (typeof DYNAMIC_KEYS)[number]; scope?: "this" };
export function settingsSearch(s: Record<string, unknown>): SettingsSearch {
  return { edit: oneOf(DYNAMIC_KEYS, s.edit), scope: oneOf(["this"] as const, s.scope) };
}

export type AccessSearch = { acl?: "new"; delete?: string; q?: string; kind?: string; perm?: "allow" | "deny"; state?: "secured" };
export function accessSearch(s: Record<string, unknown>): AccessSearch {
  return {
    acl: oneOf(["new"] as const, s.acl),
    delete: shortText(s.delete, 1200),
    q: shortText(s.q, 249),
    kind: oneOf(["topic", "group", "cluster", "transactional-id", "delegation-token", "user"] as const, s.kind),
    perm: oneOf(["allow", "deny"] as const, s.perm),
    state: oneOf(["secured"] as const, s.state),
  };
}

export const LAB_TABS = ["slow", "fail", "drop", "expire", "rebalance", "compact"] as const;
export type LabTab = (typeof LAB_TABS)[number];
// Rule numbers are numbers in the address (`?rule=12`): a string that looks like a number would be written as JSON (`"12"`).
export type LabSearch = { new?: LabTab; rule?: number; remove?: number };
export function labSearch(s: Record<string, unknown>): LabSearch {
  const id = (raw: unknown) => ((typeof raw === "string" || typeof raw === "number") && /^\d{1,9}$/.test(String(raw)) ? Number(raw) : undefined);
  return { new: oneOf(LAB_TABS, s.new), rule: id(s.rule), remove: id(s.remove) };
}
