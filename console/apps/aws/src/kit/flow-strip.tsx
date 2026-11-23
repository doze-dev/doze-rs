import { Link } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";
import { Fragment, useMemo } from "react";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../api/queries";
import { usePrincipals } from "../api/spaces";
import type { CallRecord } from "../api/types/CallRecord";
import { isConsole } from "../pages/live/search";
import { callerWords, namedKeys } from "./calls";
import { type ResourceLink, resourceHref } from "./contract";

/** One thing in the strip: a name, a link when it is a resource of the console, `self` for the resource the page is of. */
export type FlowChip = { label: string; to?: ResourceLink; self?: boolean; title?: string };
/** What comes together at one place of the strip: an optional word before it (`Wired to`), then its chips. */
export type FlowGroup = { label?: string; chips: FlowChip[]; /** Joined to the group before it by the word `and`, not by an arrow (the next pair of a strip that reads `a to b and c to d`). */ before?: "and" };

const CHIP = "inline-flex min-w-0 max-w-full items-center gap-2 rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright outline-none";

function ChipView({ c }: { c: FlowChip }) {
  if (c.to) {
    return (
      <Link to={resourceHref(c.to) as never} title={c.title} className={cn(CHIP, "hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50")} data-testid="wired-link">
        <span className="truncate">{c.label}</span>
      </Link>
    );
  }
  return (
    <span title={c.title} className={cn(CHIP, c.self && "border-primary/30 bg-pick")} data-testid={c.self ? "wired-self" : "wired-chip"} aria-current={c.self ? "page" : undefined}>
      <span className="truncate">{c.label}</span>
    </span>
  );
}

/** What feeds a resource, the resource, and what it feeds, as a row left to right: each group of chips after an arrow. A group with no
 * chips is left out, so a page draws only what its real calls and lists say. */
export function FlowStrip({ groups, className }: { groups: readonly FlowGroup[]; className?: string }) {
  const shown = groups.filter((g) => g.chips.length > 0);
  if (shown.length === 0) return null;
  return (
    <nav aria-label="Wired to" className={cn("mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4", className)} data-testid="wired-strip">
      {shown.map((g, i) => (
        <Fragment key={i}>
          {i > 0 && (g.before === "and" ? <span className="text-[13.5px] text-muted-foreground">and</span> : <ArrowRightIcon aria-hidden className="size-3.5 text-input" />)}
          {g.label && <span className="text-[13.5px] text-muted-foreground">{g.label}</span>}
          <span className="flex flex-wrap items-center gap-2">
            {g.chips.map((c) => (
              <ChipView key={`${c.label}|${c.self ? "self" : ""}`} c={c} />
            ))}
          </span>
        </Fragment>
      ))}
    </nav>
  );
}

/** The distinct callers of `calls` that made one of `actions`, newest first, as people read them (an IAM user's name, a client such
 * as `AWS CLI`, `Lambda function send-email`); this console's own calls are left out. At most `most`. */
export function useFeeders(calls: readonly CallRecord[], actions: readonly string[], most = 4): string[] {
  const principals = usePrincipals(useMemo(() => namedKeys(calls), [calls]));
  const about = useAbout().data;
  return useMemo(() => {
    const titles = new Map((about?.services ?? []).map((s) => [s.name, s.title]));
    const names = new Map((principals.data?.principals ?? []).map((p) => [p.key, p]));
    const seen: string[] = [];
    for (const c of calls) {
      if (!c.action || !actions.includes(c.action) || isConsole(c)) continue;
      const label = callerWords(c, titles, names).label;
      if (label !== "–" && !seen.includes(label)) seen.push(label);
    }
    return seen.slice(0, most);
  }, [calls, actions, most, about, principals.data]);
}
