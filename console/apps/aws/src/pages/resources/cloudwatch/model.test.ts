import { describe, expect, test } from "bun:test";
import { type Alarm, actionOf, actionsExplainer, awsService, byService, conditionWords, periodWords, recentFirst, statisticWords, toMs, transitionOf } from "./model";

const dlq: Alarm = { AlarmName: "dlq-not-empty", StateValue: "ALARM", MetricName: "DlqMessages", Namespace: "Larder/Queues", Statistic: "Maximum", Period: 60, EvaluationPeriods: 1, DatapointsToAlarm: 1, Threshold: 1, ComparisonOperator: "GreaterThanOrEqualToThreshold", StateUpdatedTimestamp: 1_762_588_333 };

describe("alarm words", () => {
  test("the condition is the board's sentence", () => {
    expect(conditionWords(dlq)).toBe("DlqMessages >= 1 for 1 datapoints within 1 minute");
    expect(conditionWords({ ...dlq, MetricName: "Errors", Period: 300, EvaluationPeriods: 3, DatapointsToAlarm: 2, Threshold: 3 })).toBe("Errors >= 3 for 2 datapoints within 15 minutes");
  });
  test("a math alarm names the expression it returns", () => {
    const math: Alarm = { AlarmName: "m", StateValue: "OK", Metrics: [{ Id: "m1", ReturnData: false, MetricStat: { Metric: { Namespace: "N", MetricName: "A" }, Period: 300, Stat: "Sum" } }, { Id: "e1", Expression: "m1*2", Label: "Doubled" }], EvaluationPeriods: 1, Threshold: 4, ComparisonOperator: "GreaterThanThreshold" };
    expect(conditionWords(math)).toBe("Doubled > 4 for 1 datapoints within 1 minute");
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
    expect(byService(rows)).toEqual([{ service: "AWS Lambda", ALARM: 0, INSUFFICIENT_DATA: 0, OK: 1 }, { service: "Amazon API Gateway", ALARM: 0, INSUFFICIENT_DATA: 1, OK: 0 }].sort((x, y) => x.service.localeCompare(y.service)));
    expect(awsService("Larder/Orders")).toBeNull();
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
    expect(actionsExplainer(dlq, [action, change]).rest).toBe("It moved from OK to ALARM at 07:52:13, so alerts was published to once; the alarm stays in ALARM without running them again.");
  });
  test("with no change it says so", () => {
    expect(actionsExplainer({ ...dlq, StateValue: "INSUFFICIENT_DATA" }, []).rest).toContain("has not changed state");
  });
});
