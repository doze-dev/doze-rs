// Create rule (eventbridge__rule-create and its steps: pattern, targets, tags, review; rule-create-schedule): the wizard, five steps for a rule with an
// event pattern and four for a scheduled one. Nothing is sent until the last step: PutRule with the tags, then PutTargets.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsValid } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, FormSection, RuleHelp, TAG_RULES, useServiceCrumbs, useTags } from "../../../kit";
import { RULES, ruleAddress, useRulesOf, useEbOp } from "./api";
import { useWhere } from "./parts";
import { RuleDetailStep } from "./rule-detail-step";
import { type RuleDraft, MOST_TARGETS, emptyRule, firstProblem, newTarget, patternProblem, putRuleInput, putTargetsInput } from "./rule-model";
import { RulePatternStep } from "./rule-pattern-step";
import { RuleReview } from "./rule-review";
import { scheduleProblem } from "./schedule-fields";
import { TargetForm } from "./target-form";
import { list } from "./words";

const STEPS = [{ id: "detail", label: "Define rule detail" }, { id: "pattern", label: "Build event pattern" }, { id: "targets", label: "Select target(s)" }, { id: "tags", label: "Configure tags" }, { id: "review", label: "Review and create" }];
const SCHEDULE_STEPS = [{ id: "detail", label: "Define rule detail and schedule" }, { id: "targets", label: "Select target(s)" }, { id: "tags", label: "Configure tags" }, { id: "review", label: "Review and create" }];
const TITLES: Record<string, [string, string]> = {
  detail: ["Create rule", "Name the rule and choose the event bus it watches."],
  pattern: ["Build event pattern", "Say which events the rule should match, and test the pattern against a sample event before you go on."],
  targets: ["Select target(s)", "Choose where matching events go, and how EventBridge should deliver them."],
  tags: ["Configure tags", "Optional labels for the rule."],
  review: ["Review and create", "Check everything, then create the rule. Edit goes back to that step."],
};

export function RuleCreate({ schedule, bus }: { schedule: boolean; bus?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [d, setD] = useState<RuleDraft>(() => ({ ...emptyRule(schedule ? "schedule" : "pattern", bus ?? "default"), tags: [newRow()] }));
  const steps = schedule ? SCHEDULE_STEPS : STEPS;
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const put = useEbOp(RULES, "PutRule");
  const targetsCall = useEbOp(RULES, "PutTargets");
  const taken = (useRulesOf(d.bus).data?.Rules ?? []).map((r) => r.Name);
  const busTags = useTags({ service: "eventbridge", kind: "buses", name: d.bus });
  const id = steps[step]!.id;
  const set = (p: Partial<RuleDraft>) => setD((x) => ({ ...x, ...p }));
  const setTarget = (key: string, p: Partial<RuleDraft["targets"][number]>) => setD((x) => ({ ...x, targets: x.targets.map((t) => (t.key === key ? { ...t, ...p } : t)) }));
  const requests = useMemo(() => [{ service: "events", operation: "PutRule", input: putRuleInput(d) }, ...(d.targets.some((t) => t.arn) ? [{ service: "events", operation: "PutTargets", input: putTargetsInput(d) }] : [])], [d]);
  const problem = firstProblem(d, taken);
  const here = (): string | null => {
    if (id === "detail") return (problem?.step === 0 ? problem.why : null) ?? (schedule ? scheduleProblem(d.schedule) : null);
    if (id === "pattern") return patternProblem(d.pattern);
    if (id === "targets") return d.targets.map((t, i) => { const p = firstProblem({ ...d, name: "x", pattern: '{"a":[1]}', targets: [t] }, []); return p ? `Target ${i + 1}: ${p.why}` : null; }).find(Boolean) ?? null;
    if (id === "tags") return rowsValid(d.tags, TAG_RULES) ? null : "Fix the tags first.";
    return null;
  };
  const move = (to: number) => {
    setStep(to);
    setFurthest((f) => Math.max(f, to));
    setTried(false);
  };
  const next = () => {
    setTried(true);
    if (here()) return;
    move(Math.min(step + 1, steps.length - 1));
  };
  const failure = put.error ?? targetsCall.error;
  const refused = failure instanceof ApiFailure ? { code: failure.code, message: failure.message } : failure ? { code: "Error", message: String((failure as Error).message) } : null;
  const made = put.isSuccess && !put.isPending;
  const submit = () => {
    const bad = schedule ? (scheduleProblem(d.schedule) ?? (problem?.step === 0 ? problem.why : null)) : problem?.why ?? null;
    if (bad || problem) {
      if (problem) move(problem.step);
      setTried(true);
      return;
    }
    const open = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "rules", name: ruleAddress(d.bus, d.name) }, search: { done: "created" } });
    const sendTargets = () => targetsCall.mutate(putTargetsInput(d) as never, { onSuccess: open });
    if (made) sendTargets();
    else put.mutate(putRuleInput(d) as never, { onSuccess: sendTargets });
  };
  const [title, sub] = TITLES[id] ?? TITLES.detail!;
  const heading = schedule && id === "detail" ? "Create scheduled rule" : title;
  const lede = schedule && id === "detail" ? "Define the rule and its schedule, then choose targets. The targets, tags and review that follow are the same as for an event pattern rule." : `Step ${step + 1} of ${steps.length}${id === "detail" && !schedule ? ", Advanced Builder" : ""}. ${sub}`;
  const bad = here();
  return (
    <FormPage
      title={heading}
      sub={lede}
      titleExtra={schedule ? <span className="rounded-full border px-2.5 py-0.5 text-[12px] text-muted-foreground">Legacy</span> : undefined}
      crumbs={[...crumbs, { label: "Rules", to: "/resources/eventbridge/rules" }, ...(id !== "detail" || schedule ? [{ label: schedule ? "Create scheduled rule (legacy)" : "Create rule", to: "/resources/eventbridge/rules?create=yes" }] : [])]}
      stepsAfterTitle
      backLabel="Previous"
      wizard={{ steps, current: step, furthest, onStep: (i) => i <= furthest && move(i), onBack: () => move(Math.max(0, step - 1)), onNext: next }}
      noCall={id !== "review"}
      request={requests[0] ?? null}
      requests={requests}
      callTitle={requests.length > 1 ? "The calls this will make" : undefined}
      refusal={refused}
      callNote={<>The rule is created first, {d.enabled ? "enabled" : "disabled"}; then its target is added. {made && failure ? <b className="font-semibold text-foreground">The rule exists; its targets were not added.</b> : "A new rule can take a short while to start matching on AWS; here it matches at once."}</>}
      banner={refused ? <Alert tone="bad" title={made ? "The rule was created, but its targets were not." : "The rule wasn't created."} testId="create-refused">{made ? "PutTargets" : "PutRule"} answered <span className="font-mono font-semibold text-bright">{refused.code}</span>: “{refused.message}”</Alert> : undefined}
      submitLabel={made ? "Add the targets" : "Create rule"}
      pending={put.isPending || targetsCall.isPending}
      disabled={id === "review" && problem !== null}
      disabledWhy={id === "review" && problem ? `${problem.why} (step ${problem.step + 1})` : undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "rules" }, search: bus ? { name: bus } : {} })}
      onSubmit={submit}
      testId="rule-create"
    >
      {bad && tried && id !== "detail" && id !== "targets" && id !== "pattern" && <p className="text-[13px] text-bad" role="alert">{bad}</p>}
      {id === "detail" && <RuleDetailStep d={d} set={set} tried={tried} />}
      {id === "pattern" && <RulePatternStep pattern={d.pattern} onPattern={(pattern) => set({ pattern })} tried={tried} />}
      {id === "targets" && (
        <>
          {d.targets.map((t, i) => <TargetForm key={t.key} t={t} index={i} set={(p) => setTarget(t.key, p)} remove={d.targets.length > 1 ? () => set({ targets: d.targets.filter((x) => x.key !== t.key) }) : undefined} single={d.targets.length === 1} region={region} account={account} tried={tried} />)}
          <div className="flex flex-wrap items-center gap-3">
            <Button type="button" variant="outline" className="bg-card" disabled={d.targets.length >= MOST_TARGETS} onClick={() => set({ targets: [...d.targets, newTarget()] })} data-testid="add-target">Add another target</Button>
            <span className="text-[13px] text-muted-foreground">{d.targets.length} of {MOST_TARGETS}</span>
          </div>
        </>
      )}
      {id === "tags" && (
        <FormSection title="Tags" optional description="A tag is a key and an optional value. Tags help you find resources and scope who may change them." testId="section-tags">
          <KeyValueEditor rows={d.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={tried} removeLabel="Remove" testId="form-tags" />
          <RuleHelp>Up to <b>50</b> tags on a rule. A key can be <b>128</b> characters and a value <b>256</b>; keys are case-sensitive and can't start with <span className="font-mono">aws:</span>. The tags are sent with the rule when it is created. On a rule that already exists, PutRule ignores tags; they change through TagResource and UntagResource.{(busTags.data?.tags.length ?? 0) > 0 ? <> <span className="text-muted-foreground">The bus {d.bus} is tagged {list((busTags.data?.tags ?? []).map((t) => (t.value ? `${t.key}=${t.value}` : t.key)))} too.</span></> : null}</RuleHelp>
        </FormSection>
      )}
      {id === "review" && <RuleReview d={d} onEdit={move} />}
    </FormPage>
  );
}
