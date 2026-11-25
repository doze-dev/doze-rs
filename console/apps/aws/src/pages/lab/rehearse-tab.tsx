import { ListChecksIcon, OctagonXIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Section } from "../card";
import { Recipes } from "./recipes-panel";
import { EVENTS, LAB_ENGINE, NEEDS_NEWER, RULES, type Rehearsal } from "./rehearsals";

/** One rehearsal: what it does, and what it stands for in production. Drawn off while this doze-aws has no engine for it. */
function RehearsalCard({ r, kind }: { r: Rehearsal; kind: "rule" | "once" }) {
  const Icon = r.icon;
  return (
    <article className="flex min-w-0 flex-col rounded-xl border bg-card p-5" data-testid={`rehearsal-${r.id}`} data-state={LAB_ENGINE ? "ready" : "off"} aria-label={r.title}>
      <div className="flex w-full items-center gap-2">
        <Icon className="size-4 shrink-0 text-warn" aria-hidden />
        <h3 className="min-w-0 text-[15px] font-semibold text-bright">{r.title}</h3>
        <Badge variant="outline" className="ml-auto shrink-0 text-[11px]">{kind === "rule" ? "rule" : "once"}</Badge>
      </div>
      <p className="mt-2 text-[13px] leading-5 text-muted-foreground">{r.lede}</p>
      <p className="mt-3 border-t pt-3 text-[12.5px] leading-[19px] text-muted-foreground">
        <b className="font-semibold text-foreground">In production: </b>
        {r.production}
      </p>
      {!LAB_ENGINE && <p className="mt-3 text-[12.5px] font-medium text-muted-foreground" data-testid="rehearsal-needs">{NEEDS_NEWER}</p>}
    </article>
  );
}

/** Rehearse: what production does to a client, made to happen here; and calls to try with your own things. */
export function RehearseTab() {
  return (
    <>
      <Recipes />
      <Section title="Rehearse a failure" testId="lab-rules" lede="A rule bends the calls your app sends to the endpoint for a while: slower, refused the way AWS refuses, or dropped. It ends by itself. The console's own calls, calls between services and calls sent from Send a call are never bent.">
        {!LAB_ENGINE && (
          <Alert tone="info" title="Rules are not in this doze-aws yet" className="mb-4" testId="lab-engine-off">
            The six rehearsals below are drawn off. They light up when doze-aws has the fault engine; nothing here changes your calls until then.
          </Alert>
        )}
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="rehearsal-cards">
          {RULES.map((r) => <RehearsalCard key={r.id} r={r} kind="rule" />)}
        </div>
      </Section>
      <Section title="Make a slow event happen now" testId="lab-events" lede="Some things production does wait on a timer here. Each of these runs the service's own step at once, never a made-up state, and says in words when there is nothing to do.">
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3" data-testid="event-cards">
          {EVENTS.map((r) => <RehearsalCard key={r.id} r={r} kind="once" />)}
        </div>
      </Section>
      <Section
        title="In force"
        testId="lab-in-force"
        lede="The rules at work now, with how many calls each touched, a link to those calls on Live, and what ended."
        actions={<Button type="button" variant="outline" size="sm" className="bg-card" disabled title={NEEDS_NEWER} data-testid="lab-stop-all"><OctagonXIcon /> Stop all</Button>}
      >
        <EmptyState icon={ListChecksIcon} title="No rules in force" className="min-h-40">
          Rules you make are listed here, each with a switch, its hits and when it ends. Stop all removes every rule at once.
        </EmptyState>
      </Section>
    </>
  );
}
