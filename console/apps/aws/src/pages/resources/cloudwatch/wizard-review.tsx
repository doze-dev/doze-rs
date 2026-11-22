// Steps 3 and 4 of the alarm wizard (cloudwatch__alarm-step3, alarm-step4): the name and description, and the review of all four.
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { Callout } from "@doze/ui/components/app/callout";
import { Field, FormSection } from "../../../kit";
import { STATES, actionOf, conditionWords, periodWords } from "./model";
import { type Draft, type Notify, type StateName, dimensionText } from "./wizard-model";

export function NameStep({ d, set, tried, problem, editing }: { d: Draft; set: (p: Partial<Draft>) => void; tried: boolean; problem: string | null; editing: boolean }) {
  return (
    <FormSection title="Name and description" testId="section-name">
      <Field label="Alarm name" error={tried ? problem ?? undefined : undefined} help={editing ? "The name of an alarm cannot be changed. Make a new alarm to get another name." : "1 to 255 characters of UTF-8, without ASCII control characters, and unique in the Region. A name cannot be changed later; copy the alarm to get a new one."}>
        {(id) => <Input id={id} value={d.name} onChange={(e) => set({ name: e.target.value })} disabled={editing} className="font-mono" autoComplete="off" spellCheck={false} data-testid="alarm-name" />}
      </Field>
      <Field label="Alarm description" optional help="Up to 1,024 characters. It is the first line of the alarm's page, as written: Markdown is kept but shown as plain text. Use it for a link to a runbook.">
        {(id) => <Textarea id={id} value={d.description} onChange={(e) => set({ description: e.target.value })} rows={5} maxLength={1024} data-testid="alarm-description" />}
      </Field>
    </FormSection>
  );
}

function Group({ title, step, go, rows, testId }: { title: string; step: number; go: (i: number) => void; rows: [string, React.ReactNode][]; testId: string }) {
  return (
    <div className="border-t pt-4 first:border-t-0 first:pt-0" data-testid={testId}>
      <div className="mb-1 flex items-center justify-between"><h3 className="text-[15px] font-semibold text-bright">{title}</h3><Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => go(step)} data-testid={`edit-${testId}`}>Edit</Button></div>
      <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] text-[14px] [&>div:last-child>*]:border-b-0">
        {rows.map(([k, v]) => <div key={k} className="contents"><dt className="border-b py-3 text-muted-foreground">{k}</dt><dd className="min-w-0 border-b py-3 break-words text-bright">{v}</dd></div>)}
      </dl>
    </div>
  );
}

const mono = (t: React.ReactNode) => <span className="font-mono text-[13px] break-all">{t}</span>;

export function ReviewStep({ d, go, arn, editing }: { d: Draft; go: (i: number) => void; arn: (n: Notify) => string; editing: boolean }) {
  const m = d.metric;
  const sample = { AlarmName: d.name, MetricName: m?.name, Namespace: m?.namespace, Statistic: d.stat, Period: d.period, EvaluationPeriods: Number(d.of), DatapointsToAlarm: Number(d.datapoints), Threshold: Number(d.threshold), ComparisonOperator: d.operator, StateValue: "OK", ThresholdMetricId: d.kind === "anomaly" ? "ad1" : undefined, Metrics: d.math ? [{ Id: "m1", ReturnData: false }, { Id: "e1", Label: d.math.label || "e1", Expression: d.math.expression }] : undefined };
  const actions = (s: StateName): React.ReactNode => {
    const list = d.notify.filter((n) => n.state === s && arn(n)).map((n) => actionOf(arn(n)));
    return list.length === 0 ? "No actions" : <span className="grid gap-2">{list.map((x) => <span key={x.arn} className="grid gap-0.5"><span>{x.text}</span>{mono(<span className="text-[12.5px] text-muted-foreground">{x.arn}</span>)}</span>)}</span>;
  };
  return (
    <FormSection title="Summary" testId="section-summary">
      <div className="grid gap-4">
        <Group testId="summary-metric" title="Specify metric and conditions" step={0} go={go} rows={[
          ["Metric", d.math ? <span key="m">Metric math over {mono(`${m?.namespace} · ${m?.name}`)}: {mono(d.math.expression)}</span> : mono(`${m?.namespace} · ${m?.name}`)],
          ...(m && m.dimensions.length ? ([["Dimensions", mono(dimensionText(m.dimensions))]] as [string, React.ReactNode][]) : []),
          ["Statistic", d.stat],
          ["Period", periodWords(d.period)],
          ["Conditions", conditionWords({ ...sample, StateValue: "OK" })],
          ["Missing data", mono(d.missing)],
        ]} />
        <Group testId="summary-actions" title="Configure actions" step={1} go={go} rows={STATES.map((s): [string, React.ReactNode] => [s, actions(s)])} />
        <Group testId="summary-name" title="Add name and description" step={2} go={go} rows={[["Name", mono(d.name)], ["Description", d.description || <span className="text-muted-foreground">None</span>]]} />
      </div>
      <Callout tone="note" testId="review-note">{editing ? "Updating the alarm replaces its settings with these. Its state and history are kept." : "Creating the alarm sets its state to INSUFFICIENT_DATA until the first evaluation."}</Callout>
    </FormSection>
  );
}
