// Create metric filter (logs__metric-filter-create, metric-filter-assign): two steps. Step 1 sets the filter pattern and tests it on sample
// messages (TestMetricFilter, which creates nothing); step 2 names the filter and says what it publishes (PutMetricFilter).
import { useMutation } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, RuleHelp, callKindOp, useKindOpRead } from "../../../kit";
import { useGroupWrite } from "./api";
import { type MetricForm, UNITS, emptyMetric, filterNameProblem, holding, messagesOf, metricInput, metricNameProblem, namespaceProblem, patternProblem, valueProblem } from "./model";

const GROUPS = { service: "logs", kind: "groups" };
const SAMPLE = '{"level":"error","app":"payments","msg":"stripe timeout","order_id":"ord_91c9"}\n{"level":"info","app":"payments","msg":"payment settled","order_id":"ord_91c9"}\n{"level":"error","app":"payments","msg":"stripe timeout","order_id":"ord_90b4"}';
type Match = { eventNumber: number; eventMessage: string };
type Filters = { metricFilters?: { filterName: string; metricTransformations?: { metricNamespace: string; metricName: string }[] }[] };

export function MetricFilterCreate({ group }: { group: string }) {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [pattern, setPattern] = useState("");
  const [text, setText] = useState(SAMPLE);
  const [f, setF] = useState<MetricForm>(emptyMetric);
  const [fresh, setFresh] = useState(true);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<MetricForm>) => setF((x) => ({ ...x, ...patch }));
  const messages = useMemo(() => messagesOf(text), [text]);
  const test = useMutation({ mutationFn: () => callKindOp<{ matches?: Match[] }>(GROUPS, "TestMetricFilter", { filterPattern: pattern, logEventMessages: messages }) });
  const put = useGroupWrite("PutMetricFilter");
  const others = useKindOpRead<Filters>(GROUPS, "DescribeMetricFilters", {});
  const existing = useMemo(() => (others.data?.metricFilters ?? []).flatMap((m) => (m.metricTransformations ?? []).map((t) => ({ name: m.filterName, namespace: t.metricNamespace, metric: t.metricName, pattern: "", value: "", unit: "" }))), [others.data]);
  const namespaces = useMemo(() => [...new Set(existing.map((e) => e.namespace))], [existing]);
  const taken = holding(existing, f.namespace);
  const p1 = patternProblem(pattern) ?? (messages.length > 50 ? "Up to 50 log events per test." : null);
  const p2 = { name: filterNameProblem(f.name), namespace: namespaceProblem(f.namespace), metric: metricNameProblem(f.metric), value: valueProblem(f.value), fallback: f.fallback.trim() !== "" && !Number.isFinite(Number(f.fallback)) ? "Enter a number." : null };
  const bad2 = Object.values(p2).find(Boolean) ?? null;
  const refused = put.error instanceof ApiFailure ? { code: put.error.code, message: put.error.message } : null;
  const back = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "logs", kind: "groups", name: group }, search: { tab: "metric-filters" } });
  const results = test.data?.matches;
  const tested = test.error ? errorWords(test.error) : null;
  const input = metricInput(group, pattern, f);
  const crumbs = [{ label: "Resources", to: "/resources" }, { label: "CloudWatch Logs", to: "/resources/logs" }, { label: "Log groups", to: "/resources/logs/groups" }, { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` }, ...(step === 1 ? [{ label: "Create metric filter", to: `/resources/logs/groups/${encodeURIComponent(group)}?view=metric-filter` }] : [])];
  const go = (i: number) => { setStep(i); setFurthest((x) => Math.max(x, i)); setTried(false); };
  return (
    <FormPage
      title={step === 0 ? "Create metric filter" : "Metric details"}
      sub={step === 0 ? <>Turn matching log events into a CloudWatch metric. <span className="text-muted-foreground">Step 1 sets the filter pattern for <span className="font-mono">{group}</span> and can test it against log event messages.</span></> : <>Name the filter and say what it publishes. <span className="text-muted-foreground">A matching log event adds the metric value to a CloudWatch metric.</span></>}
      crumbs={crumbs}
      wizard={{ steps: [{ id: "pattern", label: "Filter pattern" }, { id: "metric", label: "Metric details" }], current: step, furthest, onStep: go, onBack: () => go(0), onNext: () => { if (!p1) go(1); }, nextLabel: "Next" }}
      request={step === 0 ? { service: "logs", operation: "TestMetricFilter", input: { filterPattern: pattern, logEventMessages: messages } } : { service: "logs", operation: "PutMetricFilter", input }}
      callTitle={step === 0 ? "The call the test makes" : undefined}
      callNote={step === 0 ? "Testing creates nothing. PutMetricFilter runs when you choose Create metric filter on the next step." : "One transformation per filter."}
      refusal={refused}
      banner={refused ? <Alert tone="bad" title="Metric filter not created." testId="create-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Create metric filter"
      pending={put.isPending}
      disabled={step === 0 ? p1 !== null : tried && bad2 !== null}
      disabledWhy={step === 0 ? (p1 ?? undefined) : (bad2 ?? undefined)}
      onCancel={back}
      onSubmit={() => {
        setTried(true);
        if (bad2) return;
        put.mutate({ group, input }, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "logs", kind: "groups", name: group }, search: { tab: "metric-filters", done: "filter-created", name: f.name } }) });
      }}
      testId="metric-filter-create"
    >
      {step === 0 ? (
        <>
          <FormSection title="Filter pattern">
            <Field label="Filter pattern" error={p1 ?? undefined} help={<>Terms, <span className="font-mono">?term</span>, <span className="font-mono">-term</span> and JSON conditions like <span className="font-mono">$.msg</span>. Up to <b>1,024</b> characters. <span className="text-muted-foreground">Regular-expression and space-delimited patterns work in doze-aws within AWS's limits.</span></>}>
              {(id) => <Input id={id} value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder='{ $.msg = "stripe timeout" }' autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-pattern" />}
            </Field>
            <Field label="Log event messages" help={<>Each log event must be on one line; line breaks separate log events. Up to <b>50</b> log events per test.</>}>
              {(id) => <Textarea id={id} value={text} onChange={(e) => setText(e.target.value)} rows={4} spellCheck={false} className="bg-card font-mono text-[13px]" data-testid="mf-messages" />}
            </Field>
            <div><Button type="button" variant="outline" className="bg-card" disabled={test.isPending || messages.length === 0 || messages.length > 50} onClick={() => test.mutate()} data-testid="mf-test">{test.isPending ? "Testing…" : "Test pattern"}</Button></div>
            {tested && <Alert tone="bad" title={`TestMetricFilter was refused${tested.code ? `: ${tested.code}` : ""}`} testId="mf-test-error">{tested.message}</Alert>}
          </FormSection>
          {results && (
            <FormSection title="Filter results" description={<><b className="text-bright">{results.length} of {messages.length}</b> log event messages matched the pattern. TestMetricFilter returns the matches, each with its eventNumber and eventMessage.</>} testId="mf-results">
              {results.length === 0 ? <p className="text-[13.5px] text-muted-foreground" data-testid="mf-no-match">No message matched. Nothing would be counted.</p> : (
                <table className="w-full table-fixed border-collapse text-left text-[13px]">
                  <thead className="bg-well"><tr className="text-[11.5px] tracking-[0.07em] text-muted-foreground uppercase"><th className="w-32 rounded-l-lg px-3 py-2.5 font-medium">Event number</th><th className="rounded-r-lg px-3 py-2.5 font-medium">Event message</th></tr></thead>
                  <tbody>{results.map((m) => <tr key={m.eventNumber} className="border-b last:border-b-0"><td className="px-3 py-3 text-center font-mono tabular-nums">{m.eventNumber}</td><td className="px-3 py-3 font-mono break-all">{m.eventMessage}</td></tr>)}</tbody>
                </table>
              )}
            </FormSection>
          )}
        </>
      ) : (
        <>
          <FormSection title="Filter name">
            <Field label="Filter name" error={tried ? (p2.name ?? undefined) : undefined} help={<>Up to <b>512</b> characters, no <span className="font-mono">:</span> or <span className="font-mono">*</span>. Saving under an existing name updates that filter.</>}>
              {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="stripe-timeouts" autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-name" />}
            </Field>
          </FormSection>
          <FormSection title="Metric details">
            <div className="grid gap-5 md:grid-cols-2">
              <div className="grid content-start gap-1.5">
                <Field label="Metric namespace" error={tried ? (p2.namespace ?? undefined) : undefined}>
                  {(id) => fresh || namespaces.length === 0 ? <Input id={id} value={f.namespace} onChange={(e) => set({ namespace: e.target.value })} placeholder="Larder/Payments" autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-namespace" /> : (
                    <Select value={f.namespace} onValueChange={(v) => set({ namespace: v })}><SelectTrigger id={id} className="w-full bg-card" data-testid="mf-namespace-select"><SelectValue placeholder="Choose a namespace" /></SelectTrigger><SelectContent>{namespaces.map((n) => <SelectItem key={n} value={n}>{n}</SelectItem>)}</SelectContent></Select>
                  )}
                </Field>
                <div className="flex items-center gap-2.5">
                  <Checkbox id="mf-fresh" checked={fresh} onCheckedChange={(v) => { setFresh(v === true); set({ namespace: "" }); }} disabled={namespaces.length === 0} data-testid="mf-fresh" />
                  <Label htmlFor="mf-fresh" className="font-normal">Create new</Label>
                </div>
                <RuleHelp>A namespace groups similar metrics. If it doesn't exist yet, keep Create new selected.{f.namespace.trim() ? <> <span className="text-muted-foreground">{namespaces.includes(f.namespace) ? `${f.namespace} already holds ${taken.join(", ")}.` : `${f.namespace} doesn't exist yet${namespaces.length > 0 ? `; ${namespaces[0]} already holds ${holding(existing, namespaces[0] ?? "").join(", ")}` : ""}.`}</span></> : null}</RuleHelp>
              </div>
              <Field label="Metric name" error={tried ? (p2.metric ?? undefined) : undefined} help={<>Up to <b>255</b> characters, no <span className="font-mono">:</span>, <span className="font-mono">*</span> or <span className="font-mono">$</span>.</>}>
                {(id) => <Input id={id} value={f.metric} onChange={(e) => set({ metric: e.target.value })} placeholder="StripeTimeouts" autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-metric" />}
              </Field>
              <Field label="Metric value" error={tried ? (p2.value ?? undefined) : undefined} help={<>Enter <b>1</b> to count matching events, or a field reference like <span className="font-mono">$.latency</span> to add the number found in that field.</>}>
                {(id) => <Input id={id} value={f.value} onChange={(e) => set({ value: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-value" />}
              </Field>
              <Field label="Default value" optional error={tried ? (p2.fallback ?? undefined) : undefined} help="The value to emit when the filter pattern does not match a log event.">
                {(id) => <Input id={id} value={f.fallback} onChange={(e) => set({ fallback: e.target.value })} placeholder="Not set" autoComplete="off" spellCheck={false} className="font-mono" data-testid="mf-default" />}
              </Field>
              <Field label="Unit" optional help="Left unset, the unit is None.">
                {(id) => <Select value={f.unit || "None"} onValueChange={(v) => set({ unit: v })}><SelectTrigger id={id} className="w-full bg-card" data-testid="mf-unit"><SelectValue /></SelectTrigger><SelectContent>{UNITS.map((u) => <SelectItem key={u} value={u}>{u}</SelectItem>)}</SelectContent></Select>}
              </Field>
            </div>
          </FormSection>
        </>
      )}
    </FormPage>
  );
}
