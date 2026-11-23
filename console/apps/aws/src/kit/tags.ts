// Tags, pure: what a save changes, and the limits AWS puts on tags.
import { filled, type KeyValueRow, type KeyValueRules } from "@doze/ui/components/app/key-values";
import type { Tag, TagsChange } from "./contract";

/** Tag limits (the tagging guides of AWS: 50 tags, keys to 128 characters, values to 256, keys case-sensitive). */
export const TAG_RULES: KeyValueRules = { caseSensitive: true, maxKey: 128, maxValue: 256, maxRows: 50 };

/** The change from `before` to the rows being edited: tags that are new or have a new value are set, tags whose key is gone are
 * removed. Blank rows are ignored. Setting a key that exists with its value as it is changes nothing. */
export function diffTags(before: readonly Tag[], rows: readonly KeyValueRow[]): TagsChange {
  const now = filled(rows);
  const had = new Map(before.map((t) => [t.key, t.value]));
  const keys = new Set(now.map((r) => r.key));
  return {
    set: now.filter((r) => had.get(r.key) !== r.value).map((r) => ({ key: r.key, value: r.value })),
    remove: before.filter((t) => !keys.has(t.key)).map((t) => t.key),
  };
}

export const noChange = (c: TagsChange): boolean => c.set.length === 0 && c.remove.length === 0;

/** `3 cost allocation tags`, `1 tag`. */
export const tagWords = (n: number, noun = "tag"): string => `${n.toLocaleString("en-US")} ${noun}${n === 1 ? "" : "s"}`;

/** What a save will do, as words: `Sets 2 tags and removes 1.`, `Changes nothing.` */
export function changeWords(c: TagsChange, before: readonly Tag[]): string {
  const had = new Set(before.map((t) => t.key));
  const adds = c.set.filter((t) => !had.has(t.key)).length;
  const changes = c.set.length - adds;
  const parts = [adds > 0 && `adds ${tagWords(adds)}`, changes > 0 && `changes ${tagWords(changes)}`, c.remove.length > 0 && `removes ${tagWords(c.remove.length)}`].filter(Boolean) as string[];
  if (parts.length === 0) return "Changes nothing.";
  const text = parts.length === 1 ? parts[0]! : `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
  return `${text[0]!.toUpperCase()}${text.slice(1)}.`;
}
