// Create usage plan (apigateway__rest-usage-plan-create): a name, a throttle (a rate and a burst) and a quota (a limit per day, week or month).
// Stages and keys are added to the plan after it is made. doze-aws enforces both limits.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, RuleHelp, useServiceCrumbs } from "../../../kit";
import { PLANS, usePlans, useWrite } from "./api";
import { type PlanForm, emptyPlan, planInput, planProblem } from "./plan-model";
import type { UsagePlan } from "./types";
import { quotaWords, throttleWords } from "./words";

export function PlanCreate() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const [f, setF] = useState<PlanForm>(emptyPlan);
  const [tried, setTried] = useState(false);
  const make = useWrite<UsagePlan>(PLANS, "CreateUsagePlan");
  const have = usePlans().data?.item ?? [];
  const set = (patch: Partial<PlanForm>) => { setF((x) => ({ ...x, ...patch })); make.reset(); };
  const problem = planProblem(f);
  const input = useMemo(() => planInput(f), [f]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const example = [...have].sort((a, b) => a.name.localeCompare(b.name)).find((p) => p.throttle?.rateLimit !== undefined && p.quota?.limit !== undefined);
  return (
    <FormPage
      title="Create usage plan"
      sub="Set a rate and a quota for the keys in the plan. You add stages and API keys to the plan after you create it."
      crumbs={[...crumbs, { label: "Usage plans", to: "/resources/apigateway/usageplans" }, { label: "Create usage plan", to: "/resources/apigateway/usageplans?do=create" }]}
      request={{ service: "apigateway", operation: "CreateUsagePlan", input }}
      showInput
      refusal={failure}
      callNote="No apiStages yet: the plan covers nothing until you add a stage to it."
      banner={failure ? <Alert tone="bad" title="The usage plan wasn't created." testId="create-refused">CreateUsagePlan answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create usage plan"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "usageplans" }, search: {} })}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: (made) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "usageplans", name: made.id }, search: { done: "created" } }) }); }}
      testId="plan-create"
    >
      <FormSection title="Usage plan details" testId="section-details">
        <Field label="Name" error={tried && f.name.trim() === "" ? "Enter a name for the plan." : undefined} hint={have.length > 0 ? <>The account has {have.map((p) => p.name).join(", ")} already.</> : undefined}>{(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="partners-premium" className="font-mono" autoComplete="off" spellCheck={false} data-testid="plan-name" />}</Field>
        <Field label="Description" optional>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="plan-description" />}</Field>
      </FormSection>
      <FormSection title="Throttling" optional testId="section-throttle">
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Rate" error={tried && problem?.startsWith("Throttling") ? problem : undefined}>{(id) => <div className="flex items-center gap-3"><Input id={id} value={f.rate} onChange={(e) => set({ rate: e.target.value })} inputMode="decimal" className="w-32 font-mono" data-testid="plan-rate" /><span className="text-[13.5px] text-muted-foreground">requests per second</span></div>}</Field>
          <Field label="Burst">{(id) => <div className="flex items-center gap-3"><Input id={id} value={f.burst} onChange={(e) => set({ burst: e.target.value })} inputMode="numeric" className="w-32 font-mono" data-testid="plan-burst" /><span className="text-[13.5px] text-muted-foreground">requests</span></div>}</Field>
        </div>
        <RuleHelp>Rate is the requests per second added to the token bucket; burst is the bucket&apos;s capacity. doze-aws enforces it per key: a request over it gets 429 (<span className="font-mono">crates/aws-apigateway/src/usage.rs</span>).{example && <> {example.name} uses {throttleWords(example.throttle)}.</>}</RuleHelp>
      </FormSection>
      <FormSection title="Quota" optional testId="section-quota">
        <div className="flex flex-wrap items-end gap-4">
          <Field label="Limit">{(id) => <div className="flex items-center gap-3"><Input id={id} value={f.limit} onChange={(e) => set({ limit: e.target.value })} inputMode="numeric" className="w-36 font-mono" data-testid="plan-limit" /><span className="text-[13.5px] text-muted-foreground">requests per</span></div>}</Field>
          <Field label="Period">
            {(id) => (
              <Select value={f.period} onValueChange={(period) => set({ period: period as PlanForm["period"] })}>
                <SelectTrigger id={id} className="w-36 bg-card" data-testid="plan-period"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="DAY">Day</SelectItem><SelectItem value="WEEK">Week</SelectItem><SelectItem value="MONTH">Month</SelectItem></SelectContent>
              </Select>
            )}
          </Field>
        </div>
        <RuleHelp>The most requests one key can make in the period: day, week or month. doze-aws counts them per key by UTC day and answers 429 once the quota is used.{example && <> {example.name} allows {quotaWords(example.quota)}.</>}</RuleHelp>
      </FormSection>
    </FormPage>
  );
}
