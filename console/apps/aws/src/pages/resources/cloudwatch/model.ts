// CloudWatch's alarms as the service answers DescribeAlarms, and the words the pages say of them (the condition, the period, the
// actions). Pure: no page and no call here.

export type Dimension = { Name: string; Value: string };
export type MetricStat = { Metric: { Namespace: string; MetricName: string; Dimensions?: Dimension[] }; Period: number; Stat: string; Unit?: string };
/** One query of a metric-math alarm (or of GetMetricData): a metric, or an expression over other queries. */
export type MetricDataQuery = { Id: string; Expression?: string; Label?: string; ReturnData?: boolean; Period?: number; MetricStat?: MetricStat };

export type Alarm = {
  AlarmName: string;
  AlarmArn?: string;
  AlarmDescription?: string;
  StateValue: string;
  StateReason?: string;
  StateUpdatedTimestamp?: number | string;
  AlarmConfigurationUpdatedTimestamp?: number | string;
  ActionsEnabled?: boolean;
  AlarmActions?: string[];
  OKActions?: string[];
  InsufficientDataActions?: string[];
  MetricName?: string;
  Namespace?: string;
  Statistic?: string;
  ExtendedStatistic?: string;
  Dimensions?: Dimension[];
  Period?: number;
  Unit?: string;
  EvaluationPeriods?: number;
  DatapointsToAlarm?: number;
  Threshold?: number;
  ComparisonOperator?: string;
  TreatMissingData?: string;
  Metrics?: MetricDataQuery[];
  ThresholdMetricId?: string;
  /** A composite alarm: the rule over other alarms in place of a metric. */
  AlarmRule?: string;
};

export type HistoryItem = { AlarmName: string; Timestamp: number | string; HistoryItemType: string; HistorySummary: string; HistoryData?: string };

/** A time the service answered: epoch seconds (JSON), or text. */
export function toMs(v: number | string | undefined | null): number | null {
  if (v === undefined || v === null) return null;
  if (typeof v === "number") return v < 1e11 ? Math.round(v * 1000) : v;
  const t = Date.parse(v);
  return Number.isNaN(t) ? null : t;
}

export const OPERATORS = [
  { value: "GreaterThanThreshold", sign: ">", label: "Greater than" },
  { value: "GreaterThanOrEqualToThreshold", sign: ">=", label: "Greater than or equal to" },
  { value: "LessThanOrEqualToThreshold", sign: "<=", label: "Less than or equal to" },
  { value: "LessThanThreshold", sign: "<", label: "Less than" },
] as const;

export const signOf = (op: string | undefined): string => OPERATORS.find((o) => o.value === op)?.sign ?? op ?? "";

/** `1 minute`, `15 minutes`, `30 seconds`, `1 hour`. */
export function periodWords(seconds: number): string {
  if (seconds >= 3600 && seconds % 3600 === 0) return `${seconds / 3600} ${seconds === 3600 ? "hour" : "hours"}`;
  if (seconds >= 60 && seconds % 60 === 0) return `${seconds / 60} ${seconds === 60 ? "minute" : "minutes"}`;
  return `${seconds} ${seconds === 1 ? "second" : "seconds"}`;
}

/** The periods the metric picker offers (a multiple of 60 s, or 10 / 30 for high-resolution metrics). */
export const PERIODS = [10, 30, 60, 300, 900, 3600, 21600, 86400];

/** The query an alarm on metric math returns (the one with `ReturnData`), when it is one. */
export const returned = (a: Pick<Alarm, "Metrics">): MetricDataQuery | undefined => a.Metrics?.find((m) => m.ReturnData !== false) ?? a.Metrics?.[0];

/** The period an alarm is judged on: its own, else that of the metric behind its expression (the shortest, as AWS evaluates). */
export const periodOf = (a: Alarm): number | undefined => a.Period ?? returned(a)?.MetricStat?.Period ?? returned(a)?.Period ?? a.Metrics?.flatMap((m) => (m.MetricStat ? [m.MetricStat.Period] : [])).sort((x, y) => x - y)[0];

/** What the alarm watches, as a name: its metric, or the label (else the id) of the expression it returns. */
export function metricName(a: Alarm): string {
  if (a.MetricName) return a.MetricName;
  const q = returned(a);
  return q?.Label || q?.MetricStat?.Metric.MetricName || q?.Expression || q?.Id || "";
}

const BAND_WORDS: Record<string, string> = { GreaterThanUpperThreshold: "above", LessThanLowerThreshold: "below", LessThanLowerOrGreaterThanUpperThreshold: "outside" };

/** `DlqMessages >= 1 for 1 datapoints within 1 minute`. */
export function conditionWords(a: Alarm): string {
  const evals = a.EvaluationPeriods ?? 1;
  const period = periodOf(a) ?? 60;
  const inside = `${a.DatapointsToAlarm ?? evals} datapoints within ${periodWords(evals * period)}`;
  if (a.ThresholdMetricId) return `${metricName(a)} is ${BAND_WORDS[a.ComparisonOperator ?? ""] ?? "outside"} the band for ${inside}`;
  return `${metricName(a)} ${signOf(a.ComparisonOperator)} ${a.Threshold ?? ""} for ${inside}`;
}

/** `Maximum, 1 minute`. */
export function statisticWords(a: Alarm): string {
  const stat = a.Statistic ?? a.ExtendedStatistic ?? returned(a)?.MetricStat?.Stat ?? "";
  const period = periodOf(a);
  return period ? `${stat}, ${periodWords(period)}` : stat;
}

export type Action = { arn: string; text: string; kind: "sns" | "lambda" | "other"; name: string };

/** An alarm action read from its ARN: `Send a notification to alerts` (the topic is named), `Run the Lambda function send-email`. */
export function actionOf(arn: string): Action {
  const [, , service = "", , , ...rest] = arn.split(":");
  if (service === "sns") {
    const name = rest[0] ?? arn;
    return { arn, kind: "sns", name, text: `Send a notification to ${name}` };
  }
  if (service === "lambda") {
    const name = rest[1] ?? rest[0] ?? arn;
    return { arn, kind: "lambda", name, text: `Run the Lambda function ${name}` };
  }
  return { arn, kind: "other", name: arn, text: "Run an action doze-aws stores and does not deliver" };
}

export const STATES = ["ALARM", "OK", "INSUFFICIENT_DATA"] as const;

export const actionsOf = (a: Alarm, state: (typeof STATES)[number]): string[] => (state === "ALARM" ? a.AlarmActions : state === "OK" ? a.OKActions : a.InsufficientDataActions) ?? [];

/** The AWS service a namespace belongs to (`AWS/Lambda`: AWS Lambda), or null for a namespace of your own. */
const SERVICES: Record<string, string> = {
  "AWS/Lambda": "AWS Lambda",
  "AWS/ApiGateway": "Amazon API Gateway",
  "AWS/SQS": "Amazon SQS",
  "AWS/SNS": "Amazon SNS",
  "AWS/DynamoDB": "Amazon DynamoDB",
  "AWS/S3": "Amazon S3",
  "AWS/Kinesis": "Amazon Kinesis",
  "AWS/States": "AWS Step Functions",
  "AWS/Events": "Amazon EventBridge",
  "AWS/Logs": "Amazon CloudWatch Logs",
};
export function awsService(namespace: string | undefined): string | null {
  if (!namespace?.startsWith("AWS/")) return null;
  return SERVICES[namespace] ?? namespace.replace("AWS/", "AWS ");
}

/** The namespace of an alarm: its metric's, or that of the query it returns. */
export const namespaceOf = (a: Alarm): string | undefined => a.Namespace ?? returned(a)?.MetricStat?.Metric.Namespace;

/** The alarms of each AWS service and how many are in each state, services in order of name. */
export function byService(alarms: readonly Alarm[]): { service: string; ALARM: number; INSUFFICIENT_DATA: number; OK: number }[] {
  const out = new Map<string, { service: string; ALARM: number; INSUFFICIENT_DATA: number; OK: number }>();
  for (const a of alarms) {
    const s = awsService(namespaceOf(a));
    if (!s) continue;
    const row = out.get(s) ?? { service: s, ALARM: 0, INSUFFICIENT_DATA: 0, OK: 0 };
    if (a.StateValue === "ALARM" || a.StateValue === "OK" || a.StateValue === "INSUFFICIENT_DATA") row[a.StateValue] += 1;
    out.set(s, row);
  }
  return [...out.values()].sort((x, y) => x.service.localeCompare(y.service));
}

const RANK: Record<string, number> = { ALARM: 0, INSUFFICIENT_DATA: 1, OK: 2 };
/** Alarms with ALARM first, then by the time their state last changed, newest first. */
export function recentFirst(alarms: readonly Alarm[]): Alarm[] {
  return [...alarms].sort((a, b) => (RANK[a.StateValue] ?? 3) - (RANK[b.StateValue] ?? 3) || (toMs(b.StateUpdatedTimestamp) ?? 0) - (toMs(a.StateUpdatedTimestamp) ?? 0));
}

export const stateCounts = (alarms: readonly Alarm[]) => ({
  all: alarms.length,
  ALARM: alarms.filter((a) => a.StateValue === "ALARM").length,
  OK: alarms.filter((a) => a.StateValue === "OK").length,
  INSUFFICIENT_DATA: alarms.filter((a) => a.StateValue === "INSUFFICIENT_DATA").length,
});

/** `StateUpdate` rows say old and new state in their data (`{"oldState":{"stateValue":"OK"},"newState":{...}}`). */
export function transitionOf(item: HistoryItem): { from: string; to: string } | null {
  try {
    const d = JSON.parse(item.HistoryData ?? "{}") as { oldState?: { stateValue?: string }; newState?: { stateValue?: string } };
    const [from, to] = [d.oldState?.stateValue, d.newState?.stateValue];
    return from && to ? { from, to } : null;
  } catch {
    return null;
  }
}

const clock = (ms: number) => new Date(ms).toISOString().slice(11, 19);

/** The explainer of the Alarm actions card, from the alarm's history: what the last change of state did. */
export function actionsExplainer(a: Alarm, history: readonly HistoryItem[]): { lead: string; rest: string } {
  const lead = "An alarm runs actions only when its state changes.";
  const last = history.find((h) => h.HistoryItemType === "StateUpdate" && transitionOf(h));
  const t = last ? transitionOf(last) : null;
  if (!last || !t) return { lead, rest: `It has not changed state since it was created, so no action has run.` };
  const at = toMs(last.Timestamp) ?? 0;
  const ran = history.filter((h) => h.HistoryItemType === "Action" && h.HistorySummary.startsWith("Successfully") && Math.abs((toMs(h.Timestamp) ?? 0) - at) < 5000);
  const names = ran.map((h) => actionOf(h.HistorySummary.replace("Successfully executed action ", "")));
  const did = names.length === 0 ? "no action ran" : names.map((n) => (n.kind === "lambda" ? `${n.name} was invoked` : `${n.name} was published to`)).join(" and ");
  const stays = t.to === a.StateValue ? `; the alarm stays in ${a.StateValue} without running them again` : "";
  return { lead, rest: `It moved from ${t.from} to ${t.to} at ${clock(at)}, so ${did}${names.length ? " once" : ""}${stays}.` };
}

/** The queries that draw an alarm's metric: its own metric, or the queries of its metric math (a query that feeds another is asked for, not drawn). */
export function queriesOf(a: Alarm, label = metricName(a)): { id: string; label: string; namespace: string; metric: string; dimensions?: Record<string, string>; stat?: string; period?: number; expression?: string; hide?: boolean }[] {
  const dims = (d: Dimension[] | undefined) => Object.fromEntries((d ?? []).map((x) => [x.Name, x.Value]));
  if (a.Metrics?.length) {
    const shown = returned(a)?.Id;
    return a.Metrics.map((m) =>
      m.Expression !== undefined
        ? { id: m.Id, label: m.Label || m.Id, namespace: "", metric: "", expression: m.Expression, hide: m.Id !== shown }
        : { id: m.Id, label: m.Label || m.MetricStat?.Metric.MetricName || m.Id, namespace: m.MetricStat?.Metric.Namespace ?? "", metric: m.MetricStat?.Metric.MetricName ?? "", dimensions: dims(m.MetricStat?.Metric.Dimensions), stat: m.MetricStat?.Stat, period: m.MetricStat?.Period, hide: m.Id !== shown },
    );
  }
  if (!a.MetricName || !a.Namespace) return [];
  return [{ id: "m0", label, namespace: a.Namespace, metric: a.MetricName, dimensions: dims(a.Dimensions), stat: a.Statistic ?? a.ExtendedStatistic ?? "Average", period: a.Period }];
}
