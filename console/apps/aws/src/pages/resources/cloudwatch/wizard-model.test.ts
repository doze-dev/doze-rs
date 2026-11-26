import { describe, expect, test } from "bun:test";
import type { Alarm } from "./model";
import { type Draft, arnOf, dimensionText, empty, fromAlarm, fromSel, metricInput, nextId, parseDimensions, problem, selSearch, wholeInput } from "./wizard-model";

const draft = (over: Partial<Draft> = {}): Draft => ({ ...empty(), metric: { namespace: "Larder/Queues", name: "DlqMessages", dimensions: [{ Name: "QueueName", Value: "pick-lists-dlq" }] }, stat: "Maximum", period: 60, name: "dlq-not-empty", ...over });

describe("the call each step makes", () => {
  test("step 1 is the board's PutMetricAlarm so far", () => {
    expect(metricInput(draft())).toEqual({ Namespace: "Larder/Queues", MetricName: "DlqMessages", Dimensions: [{ Name: "QueueName", Value: "pick-lists-dlq" }], Statistic: "Maximum", Period: 60, ComparisonOperator: "GreaterThanOrEqualToThreshold", Threshold: 1, EvaluationPeriods: 1, DatapointsToAlarm: 1, TreatMissingData: "missing" });
  });
  test("a percentile is an ExtendedStatistic", () => {
    const i = metricInput(draft({ stat: "p95" }));
    expect(i.ExtendedStatistic).toBe("p95");
    expect(i.Statistic).toBeUndefined();
  });
  test("math: the metric is asked for and the expression is what the alarm watches", () => {
    const i = metricInput(draft({ math: { label: "Rate", expression: "m1 * 2" } })) as { Metrics: { Id: string; ReturnData: boolean }[]; Namespace?: string };
    expect(i.Metrics.map((m) => [m.Id, m.ReturnData])).toEqual([["m1", false], ["e1", true]]);
    expect(i.Namespace).toBeUndefined();
  });
  test("the whole input has the name first, and actions by state", () => {
    const d = draft({ notify: [{ id: 1, state: "ALARM", via: "new", arn: "", name: "alerts" }, { id: 2, state: "OK", via: "arn", arn: "arn:aws:sns:us-east-1:111122223333:ok", name: "" }], description: "Look at the DLQ." });
    const i = wholeInput(d, (n) => arnOf(n, "111122223333", "us-east-1"));
    expect(Object.keys(i)[0]).toBe("AlarmName");
    expect(i.AlarmActions).toEqual(["arn:aws:sns:us-east-1:111122223333:alerts"]);
    expect(i.OKActions).toEqual(["arn:aws:sns:us-east-1:111122223333:ok"]);
    expect(i.ActionsEnabled).toBe(true);
    expect(i.AlarmDescription).toBe("Look at the DLQ.");
  });
});

describe("what stops a step", () => {
  test("the metric and the datapoints", () => {
    expect(problem(0, empty(), [], false)).toBe("Choose the metric to watch.");
    expect(problem(0, draft({ datapoints: "3", of: "2" }), [], false)).toContain("cannot be more than");
    expect(problem(0, draft({ threshold: "" }), [], false)).toContain("threshold");
    expect(problem(0, draft(), [], false)).toBeNull();
  });
  test("a name that is taken is refused on create, not on edit", () => {
    expect(problem(2, draft(), ["dlq-not-empty"], false)).toContain("already exists");
    expect(problem(2, draft(), ["dlq-not-empty"], true)).toBeNull();
    expect(problem(2, draft({ name: " " }), [], false)).toBe("Enter the alarm name.");
  });
  test("an action is an SNS topic or a Lambda function", () => {
    const n = (arn: string) => draft({ notify: [{ id: nextId(), state: "ALARM", via: "arn", arn, name: "" }] });
    expect(problem(1, n("arn:aws:ec2:us-east-1:111122223333:instance/i-1"), [], false)).toContain("SNS topic or a Lambda");
    expect(problem(1, n("arn:aws:sns:us-east-1:111122223333:t"), [], false)).toBeNull();
  });
});

describe("an alarm back into the wizard", () => {
  const a: Alarm = { AlarmName: "x", StateValue: "OK", Namespace: "N", MetricName: "M", Dimensions: [{ Name: "d", Value: "v" }], Statistic: "Sum", Period: 300, EvaluationPeriods: 3, DatapointsToAlarm: 2, Threshold: 5, ComparisonOperator: "LessThanThreshold", AlarmActions: ["arn:aws:lambda:us-east-1:111122223333:function:f"] };
  test("its fields come back", () => {
    const d = fromAlarm(a)!;
    expect([d.stat, d.period, d.datapoints, d.of, d.threshold, d.name]).toEqual(["Sum", 300, "2", "3", "5", "x"]);
    expect(d.notify[0]).toMatchObject({ state: "ALARM", via: "lambda" });
  });
  test("a composite alarm cannot be edited here", () => expect(fromAlarm({ ...a, AlarmRule: "ALARM(a)" })).toBeNull());
  test("dimensions are typed as Name=Value", () => {
    expect(parseDimensions("QueueName=q, Env=prod")).toEqual([{ Name: "QueueName", Value: "q" }, { Name: "Env", Value: "prod" }]);
    expect(parseDimensions("nonsense")).toBeNull();
    expect(dimensionText([{ Name: "a", Value: "b" }])).toBe("a=b");
  });
});

describe("an alarm on a band", () => {
  test("is made of the metric, the band expression and the id that names the band", () => {
    const i = metricInput(draft({ kind: "anomaly", band: "3", operator: "LessThanLowerOrGreaterThanUpperThreshold" })) as { Metrics: { Id: string; Expression?: string; ReturnData: boolean }[]; ThresholdMetricId: string; Threshold?: number };
    expect(i.ThresholdMetricId).toBe("ad1");
    expect(i.Metrics.map((m) => [m.Id, m.ReturnData])).toEqual([["m1", true], ["ad1", false]]);
    expect(i.Metrics[1]?.Expression).toBe("ANOMALY_DETECTION_BAND(m1, 3)");
    expect(i.Threshold).toBeUndefined();
  });
  test("comes back into the wizard as a band", () => {
    const d = draft({ kind: "anomaly", band: "3", operator: "GreaterThanUpperThreshold" });
    const a = { AlarmName: "x", StateValue: "OK", ...metricInput(d) } as Alarm;
    const back = fromAlarm(a);
    expect([back?.kind, back?.band, back?.operator]).toEqual(["anomaly", "3", "GreaterThanUpperThreshold"]);
  });
  test("a width of 0 stops the step", () => expect(problem(0, draft({ kind: "anomaly", band: "0" }), [], false)).toContain("width"));
});

describe("a metric handed to the wizard in the address", () => {
  test("is plain parameters, nothing quoted inside anything, and comes back as the same metric", () => {
    const search = selSearch({ ns: "Larder", m: "OrdersPlaced", d: [{ Name: "Queue", Value: "a=b, c" }, { Name: "Env", Value: "prod" }], stat: "Sum", period: 60 });
    expect(search).toEqual({ ns: "Larder", metric: "OrdersPlaced", dims: ["Queue=a=b, c", "Env=prod"], stat: "Sum", period: 60 });
    expect(JSON.stringify(search)).not.toContain("\\");
    const d = fromSel(search);
    expect(d?.metric).toEqual({ namespace: "Larder", name: "OrdersPlaced", dimensions: [{ Name: "Queue", Value: "a=b, c" }, { Name: "Env", Value: "prod" }] });
    expect(d?.stat).toBe("Sum");
    expect(d?.period).toBe(60);
  });
  test("a lone dimension, a name the router read as a number, and no metric at all", () => {
    expect(fromSel({ ns: "N", metric: 404, dims: "A=1" })?.metric).toEqual({ namespace: "N", name: "404", dimensions: [{ Name: "A", Value: "1" }] });
    expect(fromSel({ ns: "N", metric: "M" })?.stat).toBe("Average");
    expect(fromSel({ ns: "N", metric: "M", stat: "Nonsense", period: -5 })?.period).toBe(300);
    expect(fromSel({})).toBeNull();
    expect(fromSel({ ns: "N" })).toBeNull();
    expect(selSearch({ ns: "N", m: "M", d: [] })).toEqual({ ns: "N", metric: "M" });
  });
});

describe("an alarm read back for editing", () => {
  test("opens a topic it notifies on the existing-topic choice, and a function on the function choice", () => {
    const base = { AlarmName: "a", MetricName: "m", Namespace: "n", Statistic: "Sum", Period: 60, EvaluationPeriods: 1, ComparisonOperator: "GreaterThanThreshold", Threshold: 1 };
    const d = fromAlarm({ ...base, AlarmActions: ["arn:aws:sns:us-east-1:111122223333:alerts", "arn:aws:lambda:us-east-1:111122223333:function:send-email"] } as never);
    expect(d?.notify.map((n) => n.via)).toEqual(["existing", "lambda"]);
    expect(d?.notify[0]?.arn).toBe("arn:aws:sns:us-east-1:111122223333:alerts");
    expect(fromAlarm(base as never)?.notify).toEqual([]);
  });
});
