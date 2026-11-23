// The create-alarm wizard's state and what it makes of it: the PutMetricAlarm input of each step and of the whole, the problems of
// a step, and an alarm read back into the state (Actions > Edit). Pure.
import { type Alarm, type Dimension, STATES, actionsOf } from "./model";

export type StateName = (typeof STATES)[number];
/** One notification: a state, and where to send it (a topic that exists, one to make, or an ARN typed; or a Lambda function). */
export type Notify = { id: number; state: StateName; via: "existing" | "new" | "arn" | "lambda"; arn: string; name: string };
export type Metric = { namespace: string; name: string; dimensions: Dimension[] };
export type Draft = {
  metric: Metric | null;
  stat: string;
  period: number;
  /** Metric math: an expression over `m1`, the metric above. */
  math: { label: string; expression: string } | null;
  /** `anomaly`: compared with a band that doze-aws works out from the metric's own data (ANOMALY_DETECTION_BAND), not a threshold. */
  kind: "static" | "anomaly";
  /** The band's width, in standard deviations. */
  band: string;
  operator: string;
  threshold: string;
  datapoints: string;
  of: string;
  missing: string;
  notify: Notify[];
  name: string;
  description: string;
};

export const STATS = ["Average", "Sum", "Minimum", "Maximum", "SampleCount", "p50", "p90", "p95", "p99"];
export const ANOMALY_OPERATORS = [
  { value: "LessThanLowerOrGreaterThanUpperThreshold", label: "Outside of the band", body: "Lower than or greater than the band" },
  { value: "GreaterThanUpperThreshold", label: "Greater than the band", body: "Above the band's upper edge" },
  { value: "LessThanLowerThreshold", label: "Lower than the band", body: "Below the band's lower edge" },
] as const;
export const MISSING = ["missing", "breaching", "notBreaching", "ignore"];

export const empty = (): Draft => ({ metric: null, stat: "Average", period: 300, math: null, kind: "static", band: "2", operator: "GreaterThanOrEqualToThreshold", threshold: "1", datapoints: "1", of: "1", missing: "missing", notify: [], name: "", description: "" });

/** A statistic as PutMetricAlarm names it: the standard ones go in `Statistic`, a percentile in `ExtendedStatistic`. */
const statMember = (stat: string) => (/^p\d/.test(stat) ? { ExtendedStatistic: stat } : { Statistic: stat });

const int = (v: string): number | null => (/^\d+$/.test(v.trim()) ? Number(v) : null);
const num = (v: string): number | null => (v.trim() !== "" && Number.isFinite(Number(v)) ? Number(v) : null);

/** The members of step 1 (the metric and the conditions). */
export function metricInput(d: Draft): Record<string, unknown> {
  const m = d.metric;
  const shared = { ComparisonOperator: d.operator, Threshold: num(d.threshold) ?? 0, EvaluationPeriods: int(d.of) ?? 1, DatapointsToAlarm: int(d.datapoints) ?? 1, TreatMissingData: d.missing };
  if (!m) return shared;
  const metric = { Namespace: m.namespace, MetricName: m.name, Dimensions: m.dimensions };
  if (d.kind === "anomaly") {
    const rest: Partial<typeof shared> = { ...shared };
    delete rest.Threshold;
    return {
      Metrics: [
        { Id: "m1", MetricStat: { Metric: metric, Period: d.period, Stat: d.stat }, ReturnData: true },
        { Id: "ad1", Expression: `ANOMALY_DETECTION_BAND(m1, ${num(d.band) ?? 2})`, Label: `${m.name} (expected)`, ReturnData: false },
      ],
      ThresholdMetricId: "ad1",
      ...rest,
    };
  }
  if (d.math) {
    return {
      Metrics: [
        { Id: "m1", MetricStat: { Metric: metric, Period: d.period, Stat: d.stat }, ReturnData: false },
        { Id: "e1", Expression: d.math.expression, Label: d.math.label || "e1", ReturnData: true },
      ],
      ...shared,
    };
  }
  return { ...metric, ...statMember(d.stat), Period: d.period, ...shared };
}

/** The actions of step 2: the ARN of each notification, by the state it is for. */
export function actionsInput(d: Draft, arns: (n: Notify) => string): Record<string, unknown> {
  const by = (s: StateName) => d.notify.filter((n) => n.state === s).map(arns).filter((a) => a !== "");
  const out: Record<string, unknown> = {};
  if (by("ALARM").length) out.AlarmActions = by("ALARM");
  if (by("OK").length) out.OKActions = by("OK");
  if (by("INSUFFICIENT_DATA").length) out.InsufficientDataActions = by("INSUFFICIENT_DATA");
  if (d.notify.length) out.ActionsEnabled = true;
  return out;
}

export const nameInput = (d: Draft): Record<string, unknown> => ({ AlarmName: d.name.trim(), ...(d.description.trim() ? { AlarmDescription: d.description.trim() } : {}) });

/** The ARN a notification names now (`new`: the one that will be made, by its name, or "" until it has one). */
export function arnOf(n: Notify, account: string, region: string): string {
  if (n.via === "new") return n.name.trim() ? `arn:aws:sns:${region}:${account}:${n.name.trim()}` : "";
  return n.arn.trim();
}

/** The whole PutMetricAlarm input, in the order the board draws it (the name first). */
export function wholeInput(d: Draft, arns: (n: Notify) => string): Record<string, unknown> {
  return { AlarmName: d.name.trim(), ...metricInput(d), ...actionsInput(d, arns), ...(d.description.trim() ? { AlarmDescription: d.description.trim() } : {}) };
}

const SNS_NAME = /^[A-Za-z0-9_-]{1,256}$/;
const ARN = /^arn:aws[a-z-]*:(sns|lambda):[a-z0-9-]*:\d{12}:.+$/;

/** What stops step `i` (0 metric, 1 actions, 2 name, 3 review) from going on, or null. `taken`: names of the alarms there are. */
export function problem(i: number, d: Draft, taken: readonly string[], editing: boolean): string | null {
  if (i === 0) {
    if (!d.metric) return "Choose the metric to watch.";
    if (d.metric.namespace.trim() === "" || d.metric.name.trim() === "") return "Choose the metric to watch.";
    if (d.math && d.math.expression.trim() === "") return "Write the expression, or remove the math.";
    if (d.kind === "anomaly" ? (num(d.band) ?? 0) <= 0 : num(d.threshold) === null) return d.kind === "anomaly" ? "Enter the band's width: a number of standard deviations above 0." : "Enter the threshold: a number.";
    const [m, n] = [int(d.datapoints), int(d.of)];
    if (m === null || n === null || m < 1 || n < 1) return "Datapoints to alarm and the number of periods are whole numbers of 1 or more.";
    if (m > n) return "Datapoints to alarm cannot be more than the number of periods it is out of.";
    return null;
  }
  if (i === 1) {
    for (const n of d.notify) {
      if (n.via === "new" && !SNS_NAME.test(n.name)) return "Name the new topic: letters, digits, hyphen and underscore, up to 256 characters.";
      if ((n.via === "existing" || n.via === "lambda") && n.arn === "") return n.via === "lambda" ? "Choose the Lambda function." : "Choose the topic to notify.";
      if (n.via === "arn" && !ARN.test(n.arn.trim())) return "An action is the ARN of an SNS topic or a Lambda function.";
    }
    for (const s of STATES) if (d.notify.filter((n) => n.state === s).length > 5) return `An alarm can have at most five actions for ${s}.`;
    return null;
  }
  if (i === 2) {
    const name = d.name.trim();
    if (name === "") return "Enter the alarm name.";
    if (name.length > 255 || [...name].some((c) => c.charCodeAt(0) < 32 || c.charCodeAt(0) === 127)) return "The name is 1 to 255 characters, without control characters.";
    if (!editing && taken.includes(name)) return `An alarm named ${name} already exists in this Region.`;
    if (d.description.length > 1024) return "The description is up to 1,024 characters.";
  }
  return null;
}

let ids = 0;
export const nextId = (): number => ++ids;

/** An alarm read into the wizard's state; `null` for one it cannot edit (a composite alarm, or metric math of another shape). */
export function fromAlarm(a: Alarm): Draft | null {
  if (a.AlarmRule !== undefined) return null;
  const base = empty();
  const common = { operator: a.ComparisonOperator ?? base.operator, threshold: String(a.Threshold ?? 1), datapoints: String(a.DatapointsToAlarm ?? a.EvaluationPeriods ?? 1), of: String(a.EvaluationPeriods ?? 1), missing: a.TreatMissingData ?? "missing", name: a.AlarmName, description: a.AlarmDescription ?? "" };
  const notify: Notify[] = STATES.flatMap((s) => actionsOf(a, s).map((arn) => ({ id: nextId(), state: s, via: arn.includes(":lambda:") ? ("lambda" as const) : ("arn" as const), arn, name: "" })));
  if (a.ThresholdMetricId) {
    const [m1, ad] = a.Metrics ?? [];
    const stat = m1?.MetricStat;
    const width = /ANOMALY_DETECTION_BAND\(\s*m1\s*(?:,\s*([\d.]+))?\s*\)/.exec(ad?.Expression ?? "")?.[1];
    if (a.Metrics?.length !== 2 || m1?.Id !== "m1" || !stat || ad?.Id !== a.ThresholdMetricId || width === undefined && !/ANOMALY_DETECTION_BAND/.test(ad.Expression ?? "")) return null;
    return { ...base, ...common, notify, kind: "anomaly", band: width ?? "2", metric: { namespace: stat.Metric.Namespace, name: stat.Metric.MetricName, dimensions: stat.Metric.Dimensions ?? [] }, stat: stat.Stat, period: stat.Period };
  }
  if (a.Metrics?.length) {
    const [m1, e1] = a.Metrics;
    const stat = m1?.MetricStat;
    if (a.Metrics.length !== 2 || m1?.Id !== "m1" || !stat || e1?.Id !== "e1" || e1.Expression === undefined) return null;
    return { ...base, ...common, notify, metric: { namespace: stat.Metric.Namespace, name: stat.Metric.MetricName, dimensions: stat.Metric.Dimensions ?? [] }, stat: stat.Stat, period: stat.Period, math: { label: e1.Label ?? "e1", expression: e1.Expression } };
  }
  if (!a.MetricName || !a.Namespace) return null;
  return { ...base, ...common, notify, metric: { namespace: a.Namespace, name: a.MetricName, dimensions: a.Dimensions ?? [] }, stat: a.Statistic ?? a.ExtendedStatistic ?? "Average", period: a.Period ?? 300 };
}

/** `Name=Value, Name2=Value2` typed as a dimension set; null when it is not one. */
export function parseDimensions(text: string): Dimension[] | null {
  const parts = text.split(",").map((p) => p.trim()).filter(Boolean);
  const out: Dimension[] = [];
  for (const p of parts) {
    const i = p.indexOf("=");
    if (i < 1 || i === p.length - 1) return null;
    out.push({ Name: p.slice(0, i).trim(), Value: p.slice(i + 1).trim() });
  }
  return out;
}

export const dimensionText = (d: readonly Dimension[]): string => d.map((x) => `${x.Name}=${x.Value}`).join(", ");


/** A metric handed to the wizard in the address, from a row of the metrics page: its namespace, name, dimensions, statistic and period. */
export type Sel = { ns: string; m: string; d: Dimension[]; stat?: string; period?: number };

/** The address search that carries a metric: plain parameters (`ns`, `metric`, `dims` as `Name=Value` each, `stat`, `period`), nothing quoted inside anything. */
export const selSearch = (s: Sel): Record<string, string | number | string[]> => ({
  ns: s.ns,
  metric: s.m,
  ...(s.d.length > 0 ? { dims: s.d.map((x) => `${x.Name}=${x.Value}`) } : {}),
  ...(s.stat ? { stat: s.stat } : {}),
  ...(s.period ? { period: s.period } : {}),
});

/** The draft an address's metric parameters make, or null when they do not name a metric. The router reads a value that looks like a number as one, so each is read as text. */
export function fromSel(search: { ns?: unknown; metric?: unknown; dims?: unknown; stat?: unknown; period?: unknown }): Draft | null {
  if (search.ns === undefined || search.metric === undefined) return null;
  const ns = String(search.ns);
  const name = String(search.metric);
  if (ns === "" || name === "") return null;
  const dims = (Array.isArray(search.dims) ? search.dims : search.dims === undefined ? [] : [search.dims]).map(String);
  const dimensions: Dimension[] = [];
  for (const d of dims) {
    const at = d.indexOf("=");
    if (at > 0) dimensions.push({ Name: d.slice(0, at), Value: d.slice(at + 1) });
  }
  const stat = typeof search.stat === "string" && STATS.includes(search.stat) ? search.stat : "Average";
  const period = typeof search.period === "number" && search.period > 0 ? search.period : 300;
  return { ...empty(), metric: { namespace: ns, name, dimensions }, stat, period };
}
