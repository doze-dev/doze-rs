import { Link } from "@tanstack/react-router";
import { CircleAlertIcon, InfoIcon, TriangleAlertIcon } from "lucide-react";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { cn } from "@doze/ui/lib/utils";
import { type Blocker, type Blockers, resourceHref } from "./contract";

/** The last word of `title` in the singular, for a count of one (`Attached policies` becomes `attached policy`). */
export function singular(title: string): string {
  const t = title.toLowerCase();
  if (t.endsWith("ies")) return `${t.slice(0, -3)}y`;
  if (t.endsWith("s") && !t.endsWith("ss")) return t.slice(0, -1);
  return t;
}

/** `1 attached policy is cleared with it`, `3 objects must be removed first`, `1 event source mapping stays behind`: a group as a
 * sentence, in the delete's own words. */
export function blockerWords(b: Blocker): string {
  const n = b.items.length;
  const title = n === 1 ? singular(b.title) : b.title.toLowerCase();
  if (b.clears) return `${n} ${title} ${n === 1 ? "is" : "are"} cleared with it`;
  if (b.blocks) return `${n} ${title} must be removed first`;
  return `${n} ${title} ${n === 1 ? "stays" : "stay"} behind`;
}

/** How a group is drawn: what the delete clears is a warning, what stops it is a stop, what it leaves is a note. */
export const groupTone = (b: Blocker): "warn" | "bad" | "info" => (b.clears ? "warn" : b.blocks ? "bad" : "info");

const TONE_BOX = { warn: "border-warn/30 bg-warn/10", bad: "border-bad/30 bg-bad/10", info: "border-cool/30 bg-cool/10" } as const;
const TONE_INK = { warn: "text-warn", bad: "text-bad", info: "text-cool" } as const;

/** What stands in the way of a delete, each kind of thing apart: those the delete clears (a warning: they go with it) and those
 * the person must deal with first (a stop). Then what survives it, so nothing is a surprise after. */
export function BlockersList({ data, loading = false, className }: { data: Blockers | undefined; loading?: boolean; className?: string }) {
  if (loading || !data) return <Skeleton className="h-20 w-full rounded-xl" data-testid="blockers-loading" />;
  if (data.blockers.length === 0 && data.kept.length === 0) return null;
  return (
    <div className={cn("grid gap-3", className)} data-slot="blockers-list" data-testid="blockers">
      {data.blockers.map((b) => {
        const tone = groupTone(b);
        const Icon = tone === "warn" ? TriangleAlertIcon : tone === "bad" ? CircleAlertIcon : InfoIcon;
        return (
          <div key={b.title} role={tone === "bad" ? "alert" : "status"} className={cn("flex gap-3 rounded-xl border p-4 text-[13.5px] leading-[21px]", TONE_BOX[tone])} data-tone={tone} data-testid={`blocker-${b.title}`}>
            <Icon aria-hidden className={cn("mt-0.5 size-5 shrink-0", TONE_INK[tone])} />
            <div className="min-w-0">
              <p className="font-semibold text-bright">{blockerWords(b)}.</p>
              <ul className="mt-1.5 flex flex-wrap gap-1.5">
                {b.items.map((it) => (
                  <li key={it.name}>
                    {it.link ? (
                      <Link to={resourceHref(it.link) as never} className="rounded-md border bg-card px-2 py-0.5 font-mono text-[12.5px] text-link hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none">
                        {it.name}
                      </Link>
                    ) : (
                      <span className="rounded-md border bg-card px-2 py-0.5 font-mono text-[12.5px] text-foreground">{it.name}</span>
                    )}
                  </li>
                ))}
                {b.more && <li className="self-center text-[12.5px] text-muted-foreground">and more</li>}
              </ul>
            </div>
          </div>
        );
      })}
      {data.kept.length > 0 && (
        <ul className="grid gap-1 border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground" data-testid="blockers-kept">
          {data.kept.map((k) => (
            <li key={k}>{k}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
