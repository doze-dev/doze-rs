// The Details tab's two cards (cloudwatch__alarm-detail): what the alarm is, and what it does in each state.
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { Card } from "../deep/parts";
import { type Alarm, type HistoryItem, STATES, actionOf, actionsExplainer, actionsOf, conditionWords, metricName, returned, statisticWords, toMs } from "./model";
import { stateTone } from "./words";

type Row = [label: string, value: React.ReactNode, testId: string];

function Rows({ rows }: { rows: Row[] }) {
  return (
    <dl className="grid grid-cols-[minmax(7rem,11rem)_minmax(0,1fr)] text-[14px] [&>div:last-child>*]:border-b-0">
      {rows.map(([k, v, id]) => (
        <div key={k} className="contents">
          <dt className="border-b py-3 pr-4 text-muted-foreground">{k}</dt>
          <dd className="min-w-0 border-b py-3 break-words text-bright" data-testid={id}>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

const mono = (t: React.ReactNode) => <span className="font-mono text-[13px] break-all">{t}</span>;
const at = (v: number | string | undefined) => { const t = toMs(v); return t === null ? "–" : dateTimeUtc(t); };

/** The metric of the alarm: its namespace, and below it each dimension (`QueueName = pick-lists-dlq`); metric math lists its queries. */
function Metric({ a }: { a: Alarm }) {
  if (a.Metrics?.length) {
    return (
      <span className="grid gap-1.5" data-testid="alarm-metric">
        <span>Metric math</span>
        {a.Metrics.map((m) => (
          <span key={m.Id} className="font-mono text-[12.5px] break-all text-muted-foreground">
            {m.Id}: {m.Expression ?? `${m.MetricStat?.Metric.Namespace} · ${m.MetricStat?.Metric.MetricName}${(m.MetricStat?.Metric.Dimensions ?? []).map((d) => ` · ${d.Name}=${d.Value}`).join("")}, ${m.MetricStat?.Stat}`}
          </span>
        ))}
      </span>
    );
  }
  return (
    <span className="grid gap-0.5" data-testid="alarm-metric">
      {mono(a.Namespace)}
      {(a.Dimensions ?? []).map((d) => <span key={d.Name} className="font-mono text-[12.5px] break-all text-muted-foreground">{d.Name} = {d.Value}</span>)}
    </span>
  );
}

export function DetailsCard({ a }: { a: Alarm }) {
  const composite = a.AlarmRule !== undefined;
  const rows: Row[] = [
    ["Type", composite ? "Composite alarm" : "Metric alarm", "alarm-type"],
    ["State", <StatusDot key="s" tone={stateTone(a.StateValue)}>{a.StateValue}</StatusDot>, "alarm-state"],
    ["State reason", a.StateReason ?? "–", "alarm-reason"],
    ["State updated", at(a.StateUpdatedTimestamp), "alarm-updated"],
    ...(composite
      ? ([["Rule", mono(a.AlarmRule), "alarm-rule"]] as Row[])
      : ([
          ["Threshold", conditionWords(a), "alarm-threshold"],
          ["Metric", <Metric key="m" a={a} />, "alarm-metric-row"],
          ["Statistic, period", returned(a)?.MetricStat || a.Statistic || a.ExtendedStatistic ? statisticWords(a) : metricName(a), "alarm-statistic"],
          ["Datapoints to alarm", `${a.DatapointsToAlarm ?? a.EvaluationPeriods ?? 1} out of ${a.EvaluationPeriods ?? 1}`, "alarm-datapoints"],
          ["Missing data", mono(a.TreatMissingData ?? "missing"), "alarm-missing"],
        ] as Row[])),
    ["ARN", mono(a.AlarmArn ?? "–"), "alarm-arn"],
  ];
  return <Card title="Details" testId="alarm-details-card"><Rows rows={rows} /></Card>;
}

export function ActionsCard({ a, history }: { a: Alarm; history: readonly HistoryItem[] }) {
  const ex = actionsExplainer(a, history);
  return (
    <Card title="Alarm actions" testId="alarm-actions-card">
      <Rows
        rows={[
          ["Actions enabled", <StatusDot key="e" tone={a.ActionsEnabled ? "ok" : "idle"}>{a.ActionsEnabled ? "Yes" : "No"}</StatusDot>, "actions-enabled"],
          ...STATES.map((s): Row => {
            const list = actionsOf(a, s).map(actionOf);
            return [
              s,
              list.length === 0 ? "No actions" : (
                <span className="grid gap-2">
                  {list.map((x) => (
                    <span key={x.arn} className="grid gap-0.5">
                      <span><Emphasis text={x.text} names={[x.name]} /></span>
                      <span className="font-mono text-[12.5px] break-all text-muted-foreground">{x.arn}</span>
                    </span>
                  ))}
                </span>
              ),
              `actions-${s.toLowerCase()}`,
            ];
          }),
        ]}
      />
      {!a.ActionsEnabled && <p className="mt-4 text-[13.5px] text-muted-foreground" data-testid="actions-off">Actions are off for this alarm: a change of state runs none of the actions above.</p>}
      <p className="mt-5 max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-foreground" data-testid="actions-explainer">
        <b className="font-semibold text-bright">{ex.lead}</b> <span className="text-muted-foreground"><Emphasis text={ex.rest} names={[...actionsOf(a, "ALARM"), ...actionsOf(a, "OK"), ...actionsOf(a, "INSUFFICIENT_DATA")].map((x) => actionOf(x).name)} /></span>
      </p>
    </Card>
  );
}
