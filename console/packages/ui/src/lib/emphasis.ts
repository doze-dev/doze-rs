// The bold fragments of a sentence: the resource names it says, and the figures it is about. One helper, so every summary
// sentence sets its names and counts the same way. Pure; the component that draws the parts is `components/app/emphasis.tsx`.

export type Part = { text: string; /** `name`: a resource's name (set in monospace); `bold`: a figure or a phrase; `null`: plain. */ kind: "name" | "bold" | null };

/** What to set apart: `names` as written (matched whole: `pick-lists` is not found inside `pick-lists-dlq`), `bold` as written or as a
 * pattern (a pattern's whole match is bold). */
export type Targets = { names?: readonly string[]; bold?: readonly (string | RegExp)[] };

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
/** A name ends where the next character could not be part of a name: `pick-lists.` ends at the full stop, `orders.fifo` does not. */
const whole = (s: string) => new RegExp(`(?<![\\w-])(?<!\\w\\.)${escape(s)}(?![\\w-])(?!\\.[\\w-])`, "g");

type Hit = { at: number; end: number; kind: "name" | "bold" };

function hits(text: string, pattern: RegExp, kind: Hit["kind"]): Hit[] {
  return [...text.matchAll(pattern)].filter((m) => m[0].length > 0).map((m) => ({ at: m.index, end: m.index + m[0].length, kind }));
}

/** `text` cut into plain parts and the parts that are set apart. Where two would overlap the one that starts first wins, and of
 * two that start together the longer. An empty target matches nothing. */
export function emphasise(text: string, targets: Targets): Part[] {
  const all: Hit[] = [
    ...(targets.names ?? []).filter(Boolean).flatMap((n) => hits(text, whole(n), "name")),
    ...(targets.bold ?? []).filter(Boolean).flatMap((b) => hits(text, typeof b === "string" ? whole(b) : new RegExp(b.source, b.flags.includes("g") ? b.flags : `${b.flags}g`), "bold")),
  ].sort((a, b) => a.at - b.at || b.end - a.end);
  const parts: Part[] = [];
  let at = 0;
  for (const h of all) {
    if (h.at < at) continue;
    if (h.at > at) parts.push({ text: text.slice(at, h.at), kind: null });
    parts.push({ text: text.slice(h.at, h.end), kind: h.kind });
    at = h.end;
  }
  if (at < text.length) parts.push({ text: text.slice(at), kind: null });
  return parts.length > 0 ? parts : [{ text, kind: null }];
}
