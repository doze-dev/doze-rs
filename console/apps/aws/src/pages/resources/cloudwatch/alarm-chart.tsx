// An alarm's graph (the top of cloudwatch__alarm-detail): its metric over a range, with the threshold drawn as a dashed line.
import { Chart } from "@doze/ui/components/app/chart";
import { type MetricQuery, periodFor, useMetricData } from "../../../kit";
import { Card } from "../deep/parts";
import { type Alarm, conditionWords, metricName, queriesOf, signOf } from "./model";
import { RangeBar, useRange } from "./range";

export function AlarmChart({ alarm }: { alarm: Alarm }) {
  const [state, patch, range] = useRange("3h");
  const floor = alarm.Period ?? 60;
  const queries: MetricQuery[] = queriesOf(alarm).map((q) => ({ ...q, period: Math.max(q.period ?? floor, periodFor(range.to - range.from)) }));
  const m = useMetricData(queries, range);
  const above = (alarm.ComparisonOperator ?? "").startsWith("Greater");
  const threshold = alarm.Threshold === undefined || alarm.ThresholdMetricId ? undefined : { value: alarm.Threshold, label: `${metricName(alarm)} ${signOf(alarm.ComparisonOperator)} ${alarm.Threshold}`, bad: above ? ("above" as const) : ("below" as const) };
  return (
    <Card title={metricName(alarm)} actions={<RangeBar state={state} onChange={patch} compact />} testId="alarm-chart">
      <Chart series={m.series.map((s) => ({ ...s, area: true }))} from={range.from} to={range.to} height={260} threshold={threshold} loading={m.isPending && queries.length > 0} aria-label={`${conditionWords(alarm)}, graphed over the range`} empty="No data points in this range. doze-aws keeps samples for 24 hours." testId="alarm-graph" />
    </Card>
  );
}
