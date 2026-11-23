import { pluralWord } from "@doze/ui/lib/format";
// Small words the SNS pages share.

/** `a`, `a and b`, `a, b and c`. */
export function list(items: readonly string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** `1 subscription`, `3 subscriptions`. */
export const count = (n: number, one: string, many = pluralWord(one)): string => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** The time of day a call was answered, UTC: `08:13:58`. */
export const clock = (ms: number): string => new Date(ms).toISOString().slice(11, 19);
