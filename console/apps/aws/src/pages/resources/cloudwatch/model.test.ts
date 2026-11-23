import { describe, expect, test } from "bun:test";
import { type Alarm, newestSavedFirst, actionOf, actionsExplainer, awsService, byService, conditionWords, periodWords, recentFirst, statisticWords, toMs, transitionOf } from "./model";

const dlq: Alarm = { AlarmName: "dlq-not-empty", StateValue: "ALARM", MetricName: "DlqMessages", Namespace: "Larder/Queues", Statistic: "Maximum", Period: 60, EvaluationPeriods: 1, DatapointsToAlarm: 1, Threshold: 1, ComparisonOperator: "GreaterThanOrEqualToThreshold", StateUpdatedTimestamp: 1_762_588_333 };

describe("alarm words", () => {
  test("the condition is the board's sentence", () => {
    expect(conditionWords(dlq)).toBe("DlqMessages >= 1 for 1 datapoints within 1 minute");
    expect(conditionWords({ ...dlq, MetricName: "Errors", Period: 300, EvaluationPeriods: 3, DatapointsToAlarm: 2, Threshold: 3 })).toBe("Errors >= 3 for 2 datapoints within 15 minutes");
  });
  test("a math alarm names the expression it returns", () => {
    const math: Alarm = { AlarmName: "m", StateValue: "OK", Metrics: [{ Id: "m1", ReturnData: false, MetricStat: { Metric: { Namespace: "N", MetricName: "A" }, Period: 300, Stat: "Sum" } }, { Id: "e1", Expression: "m1*2", Label: "Doubled" }], EvaluationPeriods: 1, Threshold: 4, ComparisonOperator: "GreaterThanThreshold" };
    expect(conditionWords(math)).toBe("Doubled > 4 for 1 datapoints within 5 minutes");
  });
  test("periods and statistics", () => {
    expect(periodWords(30)).toBe("30 seconds");
    expect(periodWords(3600)).toBe("1 hour");
    expect(periodWords(300)).toBe("5 minutes");
    expect(statisticWords(dlq)).toBe("Maximum, 1 minute");
  });
  test("an action is read from its ARN", () => {
    expect(actionOf("arn:aws:sns:us-east-1:111122223333:alerts").text).toBe("Send a notification to alerts");
    expect(actionOf("arn:aws:lambda:us-east-1:111122223333:function:send-email").name).toBe("send-email");
    expect(actionOf("arn:aws:automate:us-east-1:ec2:stop").kind).toBe("other");
  });
});

describe("alarm lists", () => {
  const rows: Alarm[] = [dlq, { ...dlq, AlarmName: "a", StateValue: "OK", Namespace: "AWS/Lambda", StateUpdatedTimestamp: 100 }, { ...dlq, AlarmName: "b", StateValue: "INSUFFICIENT_DATA", Namespace: "AWS/ApiGateway", StateUpdatedTimestamp: 200 }];
  test("services group the AWS namespaces only", () => {
    expect(byService(rows)).toEqual([{ service: "AWS Lambda", ALARM: 0, INSUFFICIENT_DATA: 0, OK: 1 }, { service: "Amazon API Gateway", ALARM: 0, INSUFFICIENT_DATA: 1, OK: 0 }].reverse());
    expect(awsService("Larder/Orders")).toBeNull();
  });
  test("services: one in ALARM first, then the one that changed most recently, then by name", () => {
    const lambda = { ...dlq, AlarmName: "l", StateValue: "OK", Namespace: "AWS/Lambda", StateUpdatedTimestamp: 300 };
    const api = { ...dlq, AlarmName: "g", StateValue: "OK", Namespace: "AWS/ApiGateway", StateUpdatedTimestamp: 200 };
    const sqs = { ...dlq, AlarmName: "q", StateValue: "ALARM", Namespace: "AWS/SQS", StateUpdatedTimestamp: 100 };
    expect(byService([api, lambda]).map((r) => r.service)).toEqual(["AWS Lambda", "Amazon API Gateway"]);
    expect(byService([api, lambda, sqs])[0]?.service).toBe("Amazon SQS");
    expect(byService([{ ...api, StateUpdatedTimestamp: 1 }, { ...lambda, StateUpdatedTimestamp: 1 }]).map((r) => r.service)).toEqual(["Amazon API Gateway", "AWS Lambda"]);
  });
  test("ALARM first, then newest", () => {
    expect(recentFirst(rows).map((a) => a.AlarmName)).toEqual(["dlq-not-empty", "b", "a"]);
  });
  test("times", () => {
    expect(toMs(1_762_588_333)).toBe(1_762_588_333_000);
    expect(toMs("2026-11-08T07:52:13Z")).toBe(Date.parse("2026-11-08T07:52:13Z"));
  });
});

describe("what the last change did", () => {
  const change = { AlarmName: "x", Timestamp: 1_762_588_333, HistoryItemType: "StateUpdate", HistorySummary: "Alarm updated from OK to ALARM", HistoryData: JSON.stringify({ oldState: { stateValue: "OK" }, newState: { stateValue: "ALARM" } }) };
  const action = { AlarmName: "x", Timestamp: 1_762_588_334, HistoryItemType: "Action", HistorySummary: "Successfully executed action arn:aws:sns:us-east-1:111122223333:alerts" };
  test("a transition is read from the data", () => {
    expect(transitionOf(change)).toEqual({ from: "OK", to: "ALARM" });
  });
  test("the explainer names what ran", () => {
    expect(actionsExplainer(dlq, [action, change]).rest).toBe("It moved from OK to ALARM at 07:52:13, so alerts was published to once; the alarm stays in ALARM without publishing again.");
  });
  test("an alarm that invokes a function, or does both, says what it does", () => {
    const fn = { ...action, HistorySummary: "Successfully executed action arn:aws:lambda:us-east-1:111122223333:function:page-me" };
    expect(actionsExplainer(dlq, [fn, change]).rest).toContain("without invoking again.");
    expect(actionsExplainer(dlq, [fn, action, change]).rest).toContain("without running them again.");
  });
  test("with no change it says so", () => {
    expect(actionsExplainer({ ...dlq, StateValue: "INSUFFICIENT_DATA" }, []).rest).toContain("has not changed state");
  });
});

describe("an alarm on a band", () => {
  const band: Alarm = { AlarmName: "b", StateValue: "OK", Metrics: [{ Id: "m1", MetricStat: { Metric: { Namespace: "N", MetricName: "Orders" }, Period: 300, Stat: "Average" }, ReturnData: true }, { Id: "ad1", Expression: "ANOMALY_DETECTION_BAND(m1, 2)", ReturnData: false }], ThresholdMetricId: "ad1", ComparisonOperator: "GreaterThanUpperThreshold", EvaluationPeriods: 2 };
  test("is named by its metric and says which side of the band", () => {
    expect(conditionWords(band)).toBe("Orders is above the band for 2 datapoints within 10 minutes");
  });
});

describe("dashboards", () => {
  test("come newest saved first, then by name", () => {
    const rows = [{ DashboardName: "b", LastModified: 100 }, { DashboardName: "c", LastModified: 300 }, { DashboardName: "a", LastModified: 100 }, { DashboardName: "z" }];
    expect(newestSavedFirst(rows).map((r) => r.DashboardName)).toEqual(["c", "a", "b", "z"]);
  });
});
