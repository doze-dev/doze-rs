import { ListChecksIcon, OctagonXIcon, Trash2Icon } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { Alert, ErrorBlock } from "@doze/ui/components/app/alert";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { plural } from "@doze/ui/lib/format";
import { useClearEnded, useClearOnce, useLabGuide, useLabRules, useOnceGuide, useOnceIndex, useRemoveRule, useStopAll, useSwitchRule } from "../../api/lab";
import type { LabOnceKind } from "../../api/types/LabOnceKind";
import { Section } from "../card";
import { OnceDialog } from "./once-dialog";
import { OnceHistory } from "./once-history";
import { OnceCards, RuleCards } from "./rehearse-cards";
import { Recipes } from "./recipes-panel";
import { formFor, isRuleTab, newForm, type RuleForm } from "./rule-form";
import { RuleDialog } from "./rule-dialog";
import { RuleSheet } from "./rule-sheet";
import { EndedTable, InForceTable } from "./rules-table";

/** How the Lab works, from the server's words: what it is for, its steps and what it does not do. */
function HowItWorks() {
  const guide = useLabGuide();
  const intro = guide.data?.intro;
  if (!intro) return null;
  return (
    <details className="mb-8 rounded-[var(--radius-card)] border bg-card px-5 py-4" data-testid="lab-intro">
      <summary className="cursor-pointer text-[14px] font-medium text-bright">{intro.what}</summary>
      <ol className="mt-3 grid gap-2.5">
        {intro.steps.map((s, i) => (
          <li key={s.title} className="text-[13.5px] leading-5"><b className="font-semibold text-bright">{i + 1}. {s.title}.</b> <span className="text-muted-foreground">{s.text}</span></li>
        ))}
      </ol>
      <ul className="mt-3 list-disc pl-5 text-[13px] leading-5 text-muted-foreground">{intro.limits.map((l) => <li key={l}>{l}</li>)}</ul>
    </details>
  );
}

/** Rehearse: the rules in force (and what ended), the six rehearsals that make a rule, recipes made from your own things, and the forced events. */
export function RehearseTab() {
  const rules = useLabRules();
  const guide = useLabGuide();
  const once = useOnceIndex();
  const onceGuide = useOnceGuide();
  const [dialog, setDialog] = useState<RuleForm | null>(null);
  const [opened, setOpened] = useState<number | null>(null);
  const [forced, setForced] = useState<LabOnceKind | null>(null);
  const flip = useSwitchRule();
  const remove = useRemoveRule();
  const stop = useStopAll();
  const clearEnded = useClearEnded();
  const clearOnce = useClearOnce();
  const data = rules.data;
  const enabled = data?.enabled === true;
  const inForce = data?.in_force ?? [];
  return (
    <>
      <HowItWorks />
      {data && !enabled && (
        <Alert tone="warn" title="Rules are off in this doze-aws" className="mb-8" testId="lab-off">
          {data.off_why ?? "The Lab was started off."} Send a call, its history and the forced events below still work.
        </Alert>
      )}
      <Section
        title="Rules in force"
        testId="lab-in-force"
        lede="What is bending your calls now, with how many each touched, a link to those calls on Live, and when it ends. A rule ends by itself."
        actions={
          <Button type="button" variant="outline" size="sm" className="bg-card" disabled={inForce.length === 0 || stop.isPending} onClick={() => stop.mutate(undefined, { onSuccess: (r) => toast.success(`Stopped ${plural(r.stopped, "rule")}`) })} data-testid="lab-stop-all">
            <OctagonXIcon /> Stop all
          </Button>
        }
      >
        {rules.isError && !data ? (
          <ErrorBlock what="The rules could not be read" error={rules.error} hint="Check that doze-aws is still running." onRetry={() => void rules.refetch()} retrying={rules.isFetching} testId="lab-rules-error" />
        ) : !data ? (
          <Skeleton className="h-40 rounded-xl" aria-label="Loading" />
        ) : inForce.length === 0 ? (
          <EmptyState icon={ListChecksIcon} title="No rules in force" className="min-h-40">
            {enabled ? "Make a rule below, or from one of your own things. It is listed here with a switch, its hits and when it ends." : "The Lab is off, so there is nothing to make."}
          </EmptyState>
        ) : (
          <InForceTable rules={inForce} onOpen={setOpened} busy={flip.isPending || remove.isPending} onSwitch={(id, on) => flip.mutate({ id, enabled: on })} onRemove={(r) => remove.mutate(r.id, { onSuccess: () => toast.success(`Removed ${r.name}`) })} />
        )}
      </Section>
      <Section title="Rehearse a failure" testId="lab-rules" lede="A rule bends the calls your app sends to the endpoint for a while: slower, refused the way AWS refuses, or dropped. The console's own calls, calls between services and calls sent from Send a call are never bent.">
        <RuleCards teach={guide.data?.rehearsals} enabled={enabled} onStart={(tab) => setDialog(newForm(isRuleTab(tab) ? tab : "throttle"))} />
      </Section>
      <Recipes onRehearse={enabled ? (r) => setDialog(formFor({ service: r.ask.service, op: r.ask.op, resource: r.resource })) : undefined} />
      <Section title="Make a slow event happen now" testId="lab-events" lede="Some things production does wait on a timer here. Each of these runs the service's own step at once, never a made-up state, and says in words when there is nothing to do.">
        {once.isError && !once.data ? <ErrorBlock what="The forced events could not be read" error={once.error} onRetry={() => void once.refetch()} retrying={once.isFetching} testId="lab-once-error" /> : <OnceCards kinds={once.data?.kinds} teach={onceGuide.data?.kinds} onOpen={setForced} />}
        {once.data && once.data.history.length > 0 && (
          <div className="mt-6">
            <h3 className="mb-3 text-[15px] font-semibold text-bright">Forced lately</h3>
            <OnceHistory history={once.data.history} kinds={once.data.kinds} clearing={clearOnce.isPending} onClear={() => clearOnce.mutate()} />
          </div>
        )}
      </Section>
      {data && data.ended.length > 0 && (
        <Section
          title="Ended"
          testId="lab-ended"
          lede={`The last ${data.limits.most_ended} rules that are over, and how each ended.`}
          actions={<Button type="button" variant="outline" size="sm" className="bg-card" disabled={clearEnded.isPending} onClick={() => clearEnded.mutate()} data-testid="lab-ended-clear"><Trash2Icon /> Clear the list</Button>}
        >
          <EndedTable rules={data.ended} onOpen={setOpened} />
        </Section>
      )}
      <RuleDialog initial={dialog} onClose={() => setDialog(null)} />
      <RuleSheet id={opened} onClose={() => setOpened(null)} />
      <OnceDialog kind={forced} teach={onceGuide.data?.kinds.find((t) => t.kind === forced?.id)} onClose={() => setForced(null)} />
    </>
  );
}
