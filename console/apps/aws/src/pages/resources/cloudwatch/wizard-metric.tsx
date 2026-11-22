// Step 1 of the alarm wizard (cloudwatch__alarm-step1): the metric (picked from the metrics that have data, or named), its
// statistic and period, metric math, and the conditions.
import { useMemo, useState } from "react";
import { Chart } from "@doze/ui/components/app/chart";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Field, FormSection, RadioCards, RuleHelp, useMetricData, useUnit } from "../../../kit";
import { useMetrics } from "./api";
import { OPERATORS, PERIODS, periodWords } from "./model";
import { RangeBar, useRange } from "./range";
import { ANOMALY_OPERATORS, type Draft, MISSING, STATS, dimensionText, parseDimensions } from "./wizard-model";

type Props = { d: Draft; set: (patch: Partial<Draft>) => void; tried: boolean; problem: string | null };

const mono = "font-mono text-[13px]";

function Picker({ d, set, tried }: Pick<Props, "d" | "set" | "tried">) {
  const q = useMetrics();
  const all = useMemo(() => q.data ?? [], [q.data]);
  const m = d.metric;
  const namespaces = useMemo(() => [...new Set(all.map((x) => x.Namespace))].sort().map((v) => ({ value: v, label: v })), [all]);
  const names = useMemo(() => [...new Set(all.filter((x) => x.Namespace === m?.namespace).map((x) => x.MetricName))].sort().map((v) => ({ value: v, label: v })), [all, m?.namespace]);
  const sets = useMemo(() => {
    const seen = new Set<string>();
    for (const x of all) if (x.Namespace === m?.namespace && x.MetricName === m?.name) seen.add(dimensionText(x.Dimensions ?? []));
    return [...seen].sort().map((v) => ({ value: v, label: v === "" ? "No dimensions" : v }));
  }, [all, m?.namespace, m?.name]);
  const patch = (p: Partial<NonNullable<Draft["metric"]>>) => set({ metric: { namespace: "", name: "", dimensions: [], ...m, ...p } });
  return (
    <div className="grid gap-4" data-testid="metric-picker">
      <Field label="Namespace" error={tried && !m?.namespace ? "Choose the metric to watch." : undefined} hint={q.isPending ? "Reading the metrics that have data…" : "Choose a namespace that has data, or type your own: an alarm may be made before its metric has any."}>
        {(id) => <Combobox id={id} pattern options={namespaces} value={m?.namespace || null} onChange={(v) => patch({ namespace: v ?? "", name: "", dimensions: [] })} placeholder="Choose a namespace" search="Search or type a namespace…" className="w-full" aria-label="Namespace" />}
      </Field>
      <Field label="Metric name">
        {(id) => <Combobox id={id} pattern options={names} value={m?.name || null} onChange={(v) => patch({ name: v ?? "", dimensions: [] })} placeholder="Choose a metric" search="Search or type a metric name…" className="w-full" aria-label="Metric name" />}
      </Field>
      <Field label="Dimensions" hint={<>The dimensions the metric was published with, as <span className="font-mono">Name=Value</span> pairs separated by commas.</>}>
        {(id) => <Combobox id={id} pattern options={sets} value={m ? dimensionText(m.dimensions) : null} onChange={(v) => patch({ dimensions: parseDimensions(v ?? "") ?? [] })} placeholder="Choose a dimension set" search="Search or type Name=Value…" className="w-full" aria-label="Dimensions" />}
      </Field>
    </div>
  );
}

function MetricGraph({ d }: { d: Draft }) {
  const [state, patch, range] = useRange("3h");
  const m = d.metric?.namespace && d.metric.name ? d.metric : null;
  const queries = m ? [{ id: "m1", label: d.math?.label || m.name, namespace: m.namespace, metric: m.name, dimensions: Object.fromEntries(m.dimensions.map((x) => [x.Name, x.Value])), stat: d.stat, period: Math.max(d.period, 60), ...(d.math ? { hide: true } : {}) }, ...(d.math ? [{ id: "e1", label: d.math.label || "e1", namespace: "", metric: "", expression: d.math.expression }] : [])] : [];
  const data = useMetricData(queries, range, Boolean(m) && (!d.math || d.math.expression.trim() !== ""));
  const unit = useUnit(queries, range, Boolean(m) && !d.math);
  const t = Number(d.threshold);
  const threshold = d.kind === "static" && d.threshold.trim() !== "" && Number.isFinite(t) ? { value: t, label: "Threshold 1", bad: d.operator.startsWith("Greater") ? ("above" as const) : ("below" as const) } : undefined;
  return (
    <div className="grid gap-3">
      <RangeBar state={state} onChange={patch} compact />
      <Chart series={data.series.map((s) => ({ ...s, area: true }))} from={range.from} to={range.to} height={220} threshold={threshold} unit={unit} loading={Boolean(m) && data.isPending} aria-label="Graph of the metric and statistic you chose" empty={m ? "No data points in this range yet. The alarm can be made without them." : "Choose a metric to graph it."} testId="wizard-graph" />
    </div>
  );
}

export function MetricStep({ d, set, tried, problem }: Props) {
  const [picking, setPicking] = useState(d.metric === null);
  const m = d.metric;
  const name = d.math?.label || m?.name || "the metric";
  return (
    <>
      <FormSection title="Metric" description="Graph of the metric and statistic you chose, last 3 hours." testId="section-metric">
        <MetricGraph d={d} />
        {picking || !m ? (
          <>
            <Picker d={d} set={set} tried={tried} />
            {m && <div><Button type="button" variant="outline" className="bg-card" onClick={() => setPicking(false)} data-testid="metric-done">Done</Button></div>}
          </>
        ) : (
          <dl className="grid grid-cols-[minmax(7rem,12rem)_minmax(0,1fr)] text-[14px]" data-testid="metric-rows">
            {[["Namespace", m.namespace], ["Metric name", m.name], ...m.dimensions.map((x) => [x.Name, x.Value])].map(([k, v]) => (
              <div key={k} className="contents"><dt className="border-b py-3 text-muted-foreground">{k}</dt><dd className={`border-b py-3 break-all text-bright ${mono}`}>{v}</dd></div>
            ))}
          </dl>
        )}
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Statistic">
            {(id) => <Select value={d.stat} onValueChange={(stat) => set({ stat })}><SelectTrigger id={id} className="w-full bg-card" data-testid="stat"><SelectValue /></SelectTrigger><SelectContent>{STATS.map((s) => <SelectItem key={s} value={s}>{s}</SelectItem>)}</SelectContent></Select>}
          </Field>
          <Field label="Period">
            {(id) => <Select value={String(d.period)} onValueChange={(p) => set({ period: Number(p) })}><SelectTrigger id={id} className="w-full bg-card" data-testid="period"><SelectValue /></SelectTrigger><SelectContent>{PERIODS.map((p) => <SelectItem key={p} value={String(p)}>{periodWords(p)}</SelectItem>)}</SelectContent></Select>}
          </Field>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" className="bg-card" onClick={() => setPicking((p) => !p)} data-testid="metric-edit">Edit</Button>
          <Button type="button" variant="outline" className="bg-card" disabled={d.kind === "anomaly"} onClick={() => set({ math: d.math ? null : { label: "", expression: "m1" } })} data-testid="math-toggle">{d.math ? "Remove math" : "Add math"}</Button>
        </div>
        {d.math && (
          <div className="grid gap-4 rounded-xl border bg-muted/30 p-4" data-testid="math-panel">
            <Field label="Label" optional hint="What the graph and the alarm call the result. It is e1 when empty.">
              {(id) => <Input id={id} value={d.math?.label ?? ""} onChange={(e) => set({ math: { label: e.target.value, expression: d.math?.expression ?? "" } })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="math-label" />}
            </Field>
            <Field label="Expression" error={tried && d.math.expression.trim() === "" ? "Write the expression, or remove the math." : undefined} hint={<>m1 is the metric above. The alarm watches the one value the expression gives (<span className="font-mono">m1 * 100</span>, <span className="font-mono">RATE(m1)</span>, <span className="font-mono">FILL(m1, 0)</span>). doze-aws works the expression out for every period, as the alarm is judged.</>}>
              {(id) => <Input id={id} value={d.math?.expression ?? ""} onChange={(e) => set({ math: { label: d.math?.label ?? "", expression: e.target.value } })} className="font-mono" autoComplete="off" spellCheck={false} data-testid="math-expression" />}
            </Field>
          </div>
        )}
        <RuleHelp><b>Period</b> is 10, 20, 30, or a multiple of 60 seconds. Use 10, 20 or 30 only for metrics published with a StorageResolution of 1; they make this a high-resolution alarm, which AWS charges more for.</RuleHelp>
      </FormSection>
      <FormSection title="Conditions" testId="section-conditions">
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Threshold type</span>
          <RadioCards<"static" | "anomaly"> name="kind" label="Threshold type" value={d.kind} onChange={(kind) => set(kind === "anomaly" ? { kind, math: null, operator: ANOMALY_OPERATORS[0].value } : { kind, operator: OPERATORS[1].value })} options={[{ value: "static", title: "Static" }, { value: "anomaly", title: "Anomaly detection", body: "Compare with a band worked out from the metric's own data: doze-aws uses a statistical stand-in for AWS's trained model." }]} />
        </div>
        {d.kind === "static" ? (
          <>
            <div className="grid gap-2">
              <span className="text-[14px] font-medium text-bright" id="op-label">Whenever {name} is...</span>
              <RadioCards<string> name="op" label={`Whenever ${name} is`} value={d.operator} onChange={(operator) => set({ operator })} options={OPERATORS.map((o) => ({ value: o.value, title: o.label, body: <><span className={mono}>{o.sign} threshold</span><br /><span className={mono}>{o.value}</span></> }))} />
            </div>
            <Field label="than..." error={tried && problem?.includes("threshold") ? problem : undefined}>
              {(id) => (
                <div className="flex flex-wrap items-center gap-3">
                  <Input id={id} value={d.threshold} onChange={(e) => set({ threshold: e.target.value })} className="w-40 font-mono" inputMode="decimal" autoComplete="off" data-testid="threshold" />
                  <span className="text-[13.5px] text-muted-foreground">Specify the threshold value.</span>
                </div>
              )}
            </Field>
          </>
        ) : (
          <>
            <div className="grid gap-2">
              <span className="text-[14px] font-medium text-bright">Whenever {name} is...</span>
              <RadioCards<string> name="op" label={`Whenever ${name} is`} value={d.operator} onChange={(operator) => set({ operator })} options={ANOMALY_OPERATORS.map((o) => ({ value: o.value, title: o.label, body: <><span>{o.body}</span><br /><span className={mono}>{o.value}</span></> }))} />
            </div>
            <Field label="Anomaly detection threshold" error={tried && problem?.includes("band") ? problem : undefined} hint="The band's width in standard deviations. A larger number is a wider band and fewer alarms.">
              {(id) => <Input id={id} value={d.band} onChange={(e) => set({ band: e.target.value })} className="w-40 font-mono" inputMode="decimal" autoComplete="off" data-testid="band" />}
            </Field>
          </>
        )}
        <div className="grid gap-4 border-t pt-5">
          <h3 className="text-[15px] font-semibold text-bright">Additional configuration</h3>
          <Field label="Datapoints to alarm" error={tried && problem?.includes("atapoints") ? problem : undefined} help={<>Equal numbers alarm on that many consecutive breaching periods. A lower first number makes an M out of N alarm.</>}>
            {(id) => (
              <div className="flex flex-wrap items-center gap-3">
                <Input id={id} value={d.datapoints} onChange={(e) => set({ datapoints: e.target.value })} className="w-24 font-mono" inputMode="numeric" autoComplete="off" aria-label="Datapoints to alarm" data-testid="datapoints" />
                <span className="text-[14px] text-muted-foreground">out of</span>
                <Input value={d.of} onChange={(e) => set({ of: e.target.value })} className="w-24 font-mono" inputMode="numeric" autoComplete="off" aria-label="Evaluation periods" data-testid="of" />
              </div>
            )}
          </Field>
          <Field label="Missing data treatment">
            {(id) => <Select value={d.missing} onValueChange={(missing) => set({ missing })}><SelectTrigger id={id} className="w-64 bg-card font-mono" data-testid="missing"><SelectValue /></SelectTrigger><SelectContent>{MISSING.map((s) => <SelectItem key={s} value={s} className="font-mono">{s}</SelectItem>)}</SelectContent></Select>}
          </Field>
          <RuleHelp><b>Missing data:</b> <span className="font-mono">breaching</span>, <span className="font-mono">notBreaching</span>, <span className="font-mono">ignore</span> or <span className="font-mono">missing</span>; leave it out and <span className="font-mono">missing</span> applies. With <span className="font-mono">missing</span>, an alarm whose evaluation range is entirely empty goes to INSUFFICIENT_DATA.</RuleHelp>
        </div>
      </FormSection>
    </>
  );
}
