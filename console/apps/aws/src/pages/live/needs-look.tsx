import { AlertTriangleIcon, BanIcon, CircleDashedIcon, CircleXIcon } from "lucide-react";
import type { CallRecord } from "../../api/types/CallRecord";
import { cn } from "@doze/ui/lib/utils";
import { resourceShown } from "./resource";

export type Look = { call: CallRecord; word: string; tone: "warn" | "refused" | "bad" | "idle"; count: number };

/** The calls that need a look, newest of each (caller, operation, resource, what went wrong) first, at most `most`: a call soft
 * IAM let through that it would refuse, a refusal, a failure, an operation not built. `callerName` says how an app is shown. */
export function looks(calls: readonly CallRecord[], most = 3): Look[] {
  const seen = new Map<string, Look>();
  for (const c of calls) {
    const word = c.would_be_denied ? "WOULD BE DENIED (soft)" : c.outcome === "not-built" ? "NOT BUILT" : c.outcome === "denied" ? "DENIED" : c.outcome === "error" ? (c.status < 500 ? "REFUSED" : "FAILED") : null;
    if (!word) continue;
    const key = `${word}|${c.caller}|${c.action}|${c.resource}`;
    const was = seen.get(key);
    if (was) was.count += 1;
    else seen.set(key, { call: c, word, tone: word.startsWith("WOULD") ? "warn" : word === "REFUSED" || word === "DENIED" ? "refused" : word === "FAILED" ? "bad" : "idle", count: 1 });
  }
  return [...seen.values()].slice(0, most);
}

const ICON = { warn: AlertTriangleIcon, refused: BanIcon, bad: CircleXIcon, idle: CircleDashedIcon };
const INK = { warn: "text-warn", refused: "text-refused", bad: "text-bad", idle: "text-muted-foreground" };

/** Needs a look: the newest calls that went wrong, one chip each, each opening its call. */
export function NeedsLook({ items, callerName, onOpen }: { items: Look[]; callerName: (c: CallRecord) => string; onOpen: (seq: number) => void }) {
  if (items.length === 0) return null;
  return (
    <section className="mb-4 flex flex-wrap items-stretch gap-3" aria-label="Needs a look" data-testid="needs-look">
      <span className="flex items-center text-[11.5px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">Needs a look</span>
      {items.map((l) => {
        const Icon = ICON[l.tone];
        return (
          <button key={l.call.seq} type="button" onClick={() => onOpen(l.call.seq)} className="flex min-w-0 max-w-[28rem] items-start gap-2.5 rounded-xl border bg-card px-3.5 py-2 text-left outline-none transition-colors hover:border-input focus-visible:ring-2 focus-visible:ring-ring" data-testid="look" data-word={l.word}>
            <Icon aria-hidden className={cn("mt-0.5 size-4 shrink-0", INK[l.tone])} />
            <span className="min-w-0 text-[12.5px]">
              <span className={cn("font-bold tracking-[0.04em]", INK[l.tone])}>{l.word}</span>
              <span className="text-muted-foreground"> · {callerName(l.call)}</span>
              {l.count > 1 && <span className="text-muted-foreground"> ×{l.count}</span>}
              <span className="mt-0.5 block truncate font-mono text-[12px] text-foreground">
                {l.call.action ?? "Unknown"} {resourceShown(l.call)}
                {l.call.error_code ? ` ${l.call.error_code}` : ""}
              </span>
            </span>
          </button>
        );
      })}
    </section>
  );
}
