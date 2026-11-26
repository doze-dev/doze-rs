// A status attribute of an item, said as the board says it: the value with a dot of the colour its meaning has. A column is a status
// column when it is named `status` or `state`; the words it holds are the usual ones, and any other word is left plain. Pure.
import type { Tone } from "@doze/ui/components/app/status-dot";

const WORDS: Record<Tone, readonly string[]> = {
  ok: ["active", "available", "complete", "completed", "delivered", "done", "enabled", "paid", "ready", "succeeded", "success"],
  warn: ["created", "new", "open", "pending", "placed", "processing", "queued", "waiting"],
  bad: ["cancelled", "canceled", "declined", "error", "failed", "rejected", "refused"],
  idle: ["archived", "disabled", "expired", "inactive", "paused"],
  info: ["shipped"],
};

/** Whether the column holds a status. */
export const isStatusColumn = (name: string): boolean => /^(status|state)$/i.test(name);

/** The tone of a status word, or null for a word that is not a usual one. */
export function statusTone(word: string): Tone | null {
  const w = word.trim().toLowerCase();
  for (const [tone, words] of Object.entries(WORDS) as [Tone, readonly string[]][]) if (words.includes(w)) return tone;
  return null;
}
