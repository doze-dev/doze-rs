import { describe, expect, test } from "bun:test";
import type { Alarm } from "./model";
import { type Draft, arnOf, dimensionText, empty, fromAlarm, metricInput, nextId, parseDimensions, problem, wholeInput } from "./wizard-model";

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
