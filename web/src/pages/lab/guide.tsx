// What the Lab teaches, drawn: How the Lab works (open on a first visit, folded once read, remembered), the recipes, a
// rehearsal's lesson in the dialog (what it stands for, what a good client does, the settings) and the "what to watch" box of a
// rule's sheet. The words are the server's (`GET /api/v1/lab/guide`, `crate::lab::teach`); nothing here writes them again.
import { Link } from "@tanstack/react-router";
import { ActivityIcon, ArrowRightIcon, BookOpenIcon, ChevronDownIcon, ClockIcon, BrushCleaningIcon, RefreshCcwIcon, TriangleAlertIcon, UnplugIcon, UserXIcon, XIcon, type LucideIcon } from "lucide-react";
import { useState } from "react";
import type { LabGuide } from "@/api/types/LabGuide";
import type { LabIntro } from "@/api/types/LabIntro";
import type { LabRecipe } from "@/api/types/LabRecipe";
import type { LabTeach } from "@/api/types/LabTeach";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Skeleton } from "@/components/ui/skeleton";
import type { LabTab } from "@/lib/search";
import { cn } from "@/lib/utils";
import { SectionHead } from "../r4/bits";

/** How each rehearsal is drawn: its tab's word and icon, and whether it is a rule or happens once. */
export const TAB_LOOK: Record<LabTab, { label: string; icon: LucideIcon; once: boolean }> = {
  slow: { label: "Slow", icon: ClockIcon, once: false },
  fail: { label: "Fail", icon: TriangleAlertIcon, once: false },
  drop: { label: "Drop", icon: UnplugIcon, once: true },
  expire: { label: "Expire", icon: UserXIcon, once: true },
  rebalance: { label: "Rebalance", icon: RefreshCcwIcon, once: true },
  compact: { label: "Compact", icon: BrushCleaningIcon, once: true },
};

/** The lesson of a tab (or of a rule's kind). */
export function teachOf(guide: LabGuide | undefined, tab: string): LabTeach | undefined {
  return guide?.rehearsals.find((t) => t.tab === tab);
}

export const GUIDE_KEY = "doze:lab-guide";

function readFolded(): boolean {
  try {
    return globalThis.localStorage?.getItem(GUIDE_KEY) === "folded";
  } catch {
    return false;
  }
}

/** How the Lab works: open until the person folds it, then one line that opens it again. Read before the first paint, so the
 * page never jumps. */
export function HowItWorks({ intro }: { intro: LabIntro | undefined }) {
  const [folded, setFolded] = useState(readFolded);
  const fold = (f: boolean) => {
    setFolded(f);
    try {
      globalThis.localStorage?.setItem(GUIDE_KEY, f ? "folded" : "open");
    } catch {
      // private mode: it stays as chosen while the page is open
    }
  };
  return (
    <Collapsible open={!folded} onOpenChange={(o) => fold(!o)} asChild>
      <section aria-labelledby="lab-guide" className="mb-8 rounded-xl border bg-card" data-testid="lab-guide" data-state={folded ? "folded" : "open"}>
        <h2 id="lab-guide" className="text-[15px] font-semibold text-bright">
          <CollapsibleTrigger className="flex w-full items-center gap-3 rounded-xl px-5 py-3.5 text-left hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="lab-guide-toggle">
            <BookOpenIcon className="size-4 text-link" aria-hidden />
            How the Lab works
            {folded && <span className="hidden truncate text-[13px] font-normal text-muted-foreground md:inline">· Nothing is saved · Only this broker · Every rule ends by itself</span>}
            <ChevronDownIcon className={cn("ml-auto size-4 text-muted-foreground transition-transform motion-reduce:transition-none", !folded && "rotate-180")} aria-hidden />
          </CollapsibleTrigger>
        </h2>
        <CollapsibleContent className="px-5 pb-5" data-testid="lab-guide-body">
          {!intro ? (
            <Skeleton className="h-40 w-full" />
          ) : (
            <>
              <p className="max-w-[60rem] text-[14px] leading-6 text-foreground">{intro.what}</p>
              <div className="mt-5 grid gap-6 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
                <ol className="grid gap-4 sm:grid-cols-3" aria-label="How to use it">
                  {intro.steps.map((s, i) => (
                    <li key={s.title} className="min-w-0">
                      <span className="inline-flex size-6 items-center justify-center rounded-full bg-primary text-[12.5px] font-semibold text-primary-foreground" aria-hidden>
                        {i + 1}
                      </span>
                      <h3 className="mt-2 text-[14px] font-medium text-bright">{s.title}</h3>
                      <p className="mt-1 text-[13px] leading-5 text-muted-foreground">{s.text}</p>
                    </li>
                  ))}
                </ol>
                <div className="min-w-0 rounded-lg bg-well p-4">
                  <h3 className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What it does not do</h3>
                  <ul className="mt-2 flex flex-col gap-2 text-[13px] leading-5">
                    {intro.limits.map((l) => (
                      <li key={l} className="flex gap-2">
                        <span className="mt-2 size-1.5 flex-none rounded-full bg-muted-foreground" aria-hidden />
                        <span>{l}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              </div>
              <div className="mt-4 flex justify-end">
                <Button variant="outline" size="sm" className="bg-card" onClick={() => fold(true)} data-testid="lab-guide-dismiss">
                  <XIcon /> Got it, fold this away
                </Button>
              </div>
            </>
          )}
        </CollapsibleContent>
      </section>
    </Collapsible>
  );
}

/** The recipes: a question about your app each, and the dialog filled in to answer it. */
export function Recipes({ recipes, onUse }: { recipes: LabRecipe[] | undefined; onUse: (r: LabRecipe, from: HTMLElement) => void }) {
  return (
    <section aria-labelledby="recipes" className="mb-12">
      <SectionHead id="recipes" title="Recipes" sub="Each asks a question about your app and fills in the rehearsal that answers it, with this broker's own topics, apps and groups. You review it, then start it." />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="recipes">
        {!recipes
          ? Array.from({ length: 6 }, (_, i) => <Skeleton key={i} className="h-[148px] rounded-xl" />)
          : recipes.map((r) => {
              const look = TAB_LOOK[r.tab as LabTab];
              const Icon = look?.icon ?? ActivityIcon;
              return (
                <button
                  key={r.id}
                  type="button"
                  disabled={!!r.needs}
                  onClick={(ev) => onUse(r, ev.currentTarget)}
                  className="group flex min-w-0 flex-col rounded-xl border bg-card p-5 text-left transition-colors hover:border-primary/40 hover:bg-accent/30 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none disabled:cursor-not-allowed disabled:hover:border-border disabled:hover:bg-card"
                  data-testid={`recipe-${r.id}`}
                >
                  <span className="flex items-center gap-2 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">
                    <Icon className="size-3.5 text-warn" aria-hidden /> {look?.label ?? r.tab}
                  </span>
                  <span className={cn("mt-2 text-[14.5px] leading-[21px] font-semibold break-words", r.needs ? "text-muted-foreground" : "text-bright")}>{r.title}</span>
                  <span className="mt-1.5 text-[13px] leading-5 text-muted-foreground">{r.question}</span>
                  <span className="mt-auto pt-3 text-[13px] font-medium">
                    {r.needs ? (
                      <span className="text-muted-foreground">{r.needs}</span>
                    ) : (
                      <span className="inline-flex items-center gap-1 text-link">
                        Use this recipe <ArrowRightIcon className="size-3.5 transition-transform group-hover:translate-x-0.5 motion-reduce:transition-none" aria-hidden />
                      </span>
                    )}
                  </span>
                </button>
              );
            })}
      </div>
    </section>
  );
}

/** A rehearsal's lesson in the dialog: what it stands for, and on demand what a well-behaved client does, the settings that
 * decide it and where to see it here. */
export function Lesson({ teach }: { teach: LabTeach | undefined }) {
  const [open, setOpen] = useState(false);
  if (!teach) return <Skeleton className="h-16 w-full" />;
  return (
    <Collapsible open={open} onOpenChange={setOpen} className="rounded-lg border bg-well" data-testid="lab-lesson">
      <div className="flex flex-wrap items-start gap-x-4 gap-y-2 p-3">
        <p className="min-w-0 flex-1 basis-[22rem] text-[13px] leading-5">
          <b className="font-semibold text-bright">In production: </b>
          {teach.simulates}
        </p>
        <CollapsibleTrigger asChild>
          <Button type="button" variant="ghost" size="sm" className="-my-1 text-link hover:text-link" data-testid="lab-lesson-toggle">
            {open ? "Less" : "How a good app copes"}
            <ChevronDownIcon className={cn("transition-transform motion-reduce:transition-none", open && "rotate-180")} aria-hidden />
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent className="border-t px-3 pt-3 pb-4 text-[13px] leading-5" data-testid="lab-lesson-body">
        <div className="grid gap-5 md:grid-cols-2">
          <div className="min-w-0">
            <h3 className="font-semibold text-bright">A well-behaved app</h3>
            <ul className="mt-1.5 flex list-disc flex-col gap-1 pl-4 marker:text-muted-foreground">
              {teach.client.map((c) => (
                <li key={c}>{c}</li>
              ))}
            </ul>
            <h3 className="mt-4 font-semibold text-bright">Where to see it</h3>
            <p className="mt-1.5 text-muted-foreground">{teach.watch}</p>
          </div>
          <div className="min-w-0">
            <h3 className="font-semibold text-bright">Settings that decide it</h3>
            <dl className="mt-1.5 flex flex-col gap-2">
              {teach.settings.map((s) => (
                <div key={s.name} className="min-w-0">
                  <dt className="flex flex-wrap items-baseline gap-x-2">
                    <code className="font-mono text-[12.5px] break-all text-bright">{s.name}</code>
                    <span className="text-[12px] text-muted-foreground">
                      {s.side}, default <code className="font-mono">{s.default}</code>
                    </span>
                  </dt>
                  <dd className="text-muted-foreground">{s.why}</dd>
                </div>
              ))}
            </dl>
            <p className="mt-3 text-[12px] text-muted-foreground">Defaults of the Java client and the broker (Kafka 4); other clients name some of them differently.</p>
          </div>
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

/** What to watch while a rule runs, with the way to see it on Live already filtered to the rule. */
export function WatchBox({ teach, rule, live }: { teach: LabTeach | undefined; rule: number; live: boolean }) {
  return (
    <div className="flex flex-col gap-3 rounded-lg border bg-card p-4" data-testid="lab-watch">
      <h3 className="text-[15px] font-medium text-bright">What to watch</h3>
      {teach ? <p className="text-[13px] leading-5 text-muted-foreground">{teach.watch}</p> : <Skeleton className="h-10 w-full" />}
      <div className="flex flex-wrap gap-2">
        <Button variant={live ? "default" : "outline"} size="sm" className={cn(!live && "bg-card")} asChild>
          <Link to="/live" search={{ lab: rule }} data-testid="watch-live">
            <ActivityIcon /> See its requests on Live
          </Link>
        </Button>
      </div>
    </div>
  );
}
