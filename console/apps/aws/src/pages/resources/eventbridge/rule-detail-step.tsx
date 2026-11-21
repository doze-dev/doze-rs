// Step 1 of the rule wizard (eventbridge__rule-create, rule-create-schedule): the name, the bus, and what kind of rule it is; for a scheduled rule, also its
// state and schedule.
import { Link } from "@tanstack/react-router";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { Callout } from "@doze/ui/components/app/callout";
import { BusPicker, Field, FormSection, RadioCards, RuleHelp } from "../../../kit";
import { useRulesOf } from "./api";
import type { RuleDraft } from "./rule-model";
import { nameProblem } from "./rule-model";
import { ScheduleFields } from "./schedule-fields";
import { list } from "./words";

export function RuleDetailStep({ d, set, tried }: { d: RuleDraft; set: (p: Partial<RuleDraft>) => void; tried: boolean }) {
  const rules = useRulesOf(d.bus);
  const taken = (rules.data?.Rules ?? []).map((r) => r.Name);
  const schedule = d.kind === "schedule";
  const problem = nameProblem(d.name) ?? (taken.includes(d.name) ? `${d.name} is already on ${d.bus}.` : null);
  return (
    <>
      <FormSection title="Rule detail" testId="section-detail">
        <Field label="Name" error={tried ? (problem ?? undefined) : undefined} hint={taken.length > 0 ? <><b className="font-mono font-semibold text-foreground">{list(taken.slice(0, 2))}</b> {taken.length === 1 ? "is" : "are"} already on {d.bus}{taken.length > 2 ? `, with ${taken.length - 2} more` : ""}.</> : undefined} help={<><b>1 to 64</b> characters: letters, digits, dot, hyphen and underscore. Two rules on the same bus can't share a name.</>}>
          {(id) => <Input id={id} value={d.name} onChange={(e) => set({ name: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="rule-name" />}
        </Field>
        <Field label="Description" optional help={<>Up to <b>512</b> characters.</>}>{(id) => <Input id={id} value={d.description} onChange={(e) => set({ description: e.target.value })} maxLength={512} autoComplete="off" data-testid="rule-description" />}</Field>
        {schedule ? (
          <>
            <RuleHelp>Scheduled rules can only be created on the <b>default</b> event bus.</RuleHelp>
            <div className="flex items-start gap-3">
              <Switch id="rule-enabled" checked={d.enabled} onCheckedChange={(enabled) => set({ enabled })} className="mt-0.5" data-testid="rule-enabled" />
              <div className="grid gap-0.5"><Label htmlFor="rule-enabled">Enable the scheduled rule</Label><span className="text-[13px] text-muted-foreground">Takes effect as soon as you create it.</span></div>
            </div>
          </>
        ) : (
          <>
            <Field label="Event bus" testId="field-bus" help={<>The rule only sees events sent to this bus. Events from AWS services always go to the <b>default</b> bus.</>}>{(id) => <BusPicker id={id} value={d.bus} onChange={(bus) => bus && set({ bus })} />}</Field>
            <div className="grid gap-1.5">
              <Label>Rule type</Label>
              <RadioCards<"pattern"> name="rule-type" label="Rule type" value="pattern" onChange={() => undefined} options={[{ value: "pattern", title: "Rule with an event pattern", body: "Runs when an event matches the pattern you write in the next step." }]} className="sm:grid-cols-1" />
            </div>
            <Callout tone="note" testId="legacy-note">Rules that run on a schedule are a legacy feature. AWS recommends EventBridge Scheduler for those, which doze-aws has too; the console also creates them as <Link to="/resources/$service/$second" params={{ service: "eventbridge", second: "rules" }} search={{ do: "schedule" }} className="text-link hover:underline" data-testid="schedule-link">Scheduled rule (legacy)</Link>. AWS first shows a rule builder selection page (Enhanced Builder or Advanced Builder); doze-aws starts at the Advanced Builder.</Callout>
          </>
        )}
      </FormSection>
      {schedule && <ScheduleFields value={d.schedule} onChange={(schedule) => set({ schedule })} tried={tried} />}
    </>
  );
}
