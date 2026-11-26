import { describe, expect, test } from "bun:test";
import type { RuleItem, TargetItem } from "./types";
import { inputNote, prettyPattern, archivesSentence, ageWords, destsSentence, inputWords, patternWords, replaysSentence, retentionWords, retryWords, rulesSentence, targetWords, windowWords } from "./words";

const A = "arn:aws:events:us-east-1:111122223333";

describe("how a target reads", () => {
  test("by its kind and name", () => {
    expect(targetWords("arn:aws:sns:us-east-1:111122223333:orders-created")).toBe("SNS topic orders-created");
    expect(targetWords("arn:aws:sqs:us-east-1:111122223333:pick-lists")).toBe("SQS queue pick-lists");
    expect(targetWords("arn:aws:lambda:us-east-1:111122223333:function:notify")).toBe("Lambda function notify");
    expect(targetWords("arn:aws:logs:us-east-1:111122223333:log-group:/aws/events:*")).toBe("Log group /aws/events");
    expect(targetWords(`${A}:api-destination/finance-webhook/2f8c`)).toBe("API destination finance-webhook");
    expect(targetWords(`${A}:event-bus/larder`)).toBe("Event bus larder");
  });
  test("the input and the retry policy", () => {
    const t: TargetItem = { Id: "a", Arn: "x", InputTransformer: { InputPathsMap: { order_id: "$.a", customer: "$.b" }, InputTemplate: "{}" }, RetryPolicy: { MaximumRetryAttempts: 20, MaximumEventAgeInSeconds: 3600 } };
    expect(inputWords(t)).toBe("Input transformer order_id, customer");
    expect(retryWords(t)).toBe("1 hour up to 20 attempts");
    expect(inputWords({ Id: "a", Arn: "x" })).toBe("Matched event");
    expect(inputWords({ Id: "a", Arn: "x", InputPath: "$.detail" })).toBe("Part of the event $.detail");
    expect(retryWords({ Id: "a", Arn: "x" })).toBe("Default");
    expect(ageWords(90)).toBe("90 s");
    expect(ageWords(1800)).toBe("30 min");
  });
});

describe("a pattern in a table", () => {
  test("names its source and detail type, or says what it is", () => {
    expect(patternWords('{"source":["larder.orders"],"detail-type":["OrderPlaced"]}')).toBe("larder.orders OrderPlaced");
    expect(patternWords('{"detail":{"a":[1]}}')).toBe("Custom pattern");
    expect(patternWords("nope")).toBe("Custom pattern");
    expect(patternWords(undefined, "cron(0 2 * * ? *)")).toBe("cron(0 2 * * ? *)");
    expect(patternWords(undefined)).toBe("–");
  });
});

describe("retention and windows", () => {
  test("a number of days, or indefinite", () => {
    expect([retentionWords(30), retentionWords(1), retentionWords(0), retentionWords(undefined)]).toEqual(["30 days", "1 day", "Indefinite", "Indefinite"]);
  });
  test("what a delivery's input was made of", () => {
    expect(inputNote(undefined)).toBeNull();
    expect(inputNote({ Id: "t", Arn: "a" })).toBeNull();
    expect(inputNote({ Id: "t", Arn: "a", InputPath: "$.detail" })).toBe("part of the event");
    expect(inputNote({ Id: "t", Arn: "a", Input: "{}" })).toBe("constant input");
    expect(inputNote({ Id: "t", Arn: "a", InputTransformer: { InputPathsMap: { x: "$.x" }, InputTemplate: "<x>" } })).toBe("input transformer, 1 variable");
  });
  test("a replay's window in UTC", () => expect(windowWords(Date.UTC(2026, 10, 6, 18) / 1000, Date.UTC(2026, 10, 7, 18) / 1000, Date.UTC(2026, 10, 22))).toBe("Nov 6 18:00 to Nov 7 18:00"));
  test("a window that leaves this year says the years", () => {
    expect(windowWords(Date.UTC(2020, 0, 1) / 1000, Date.UTC(2030, 0, 1) / 1000, Date.UTC(2026, 10, 22))).toBe("Jan 1 2020 00:00 to Jan 1 2030 00:00");
    expect(windowWords(Date.UTC(2025, 11, 31, 23) / 1000, Date.UTC(2026, 0, 1, 1) / 1000, Date.UTC(2026, 10, 22))).toBe("Dec 31 2025 23:00 to Jan 1 2026 01:00");
  });
});

describe("the sentences over the lists", () => {
  const rules: RuleItem[] = [{ Name: "order-placed", State: "ENABLED" }, { Name: "payment-settled", State: "ENABLED" }];
  const t = (arn: string): TargetItem[] => [{ Id: "t", Arn: arn }];
  test("rules: how many, enabled, targets, where the first sends", () => {
    expect(rulesSentence("larder", rules, { "order-placed": t("arn:aws:sns:us-east-1:1:orders-created"), "payment-settled": t("arn:aws:sqs:us-east-1:1:pick-lists") })).toBe("The larder bus has 2 rules, both enabled, with 2 targets between them. order-placed sends to the SNS topic orders-created.");
  });
  test("rules: none, and some disabled, and targets not read yet", () => {
    expect(rulesSentence("larder-audit", [], {})).toBe("The larder-audit bus has no rules. Events sent to it are accepted and then match nothing.");
    expect(rulesSentence("larder", [{ Name: "a", State: "ENABLED" }, { Name: "b", State: "DISABLED" }, { Name: "c", State: "ENABLED" }], {})).toBe("The larder bus has 3 rules, 2 enabled.");
  });
  test("archives, replays, destinations", () => {
    const base = { ArchiveName: "all", EventSourceArn: `${A}:event-bus/larder`, EventCount: 10 };
    expect(archivesSentence([base, { ...base, ArchiveName: "pay", EventCount: 5, RetentionDays: 90, EventPattern: '{"source":["larder.payments"]}' }])).toBe("2 archives hold 15 events from larder. pay keeps only larder.payments events, for 90 days.");
    expect(replaysSentence([{ ReplayName: "a", EventSourceArn: "x", State: "COMPLETED" }, { ReplayName: "b", EventSourceArn: "x", State: "COMPLETED" }])).toBe("2 replays, all completed.");
    expect(replaysSentence([{ ReplayName: "a", EventSourceArn: "x", State: "COMPLETED" }, { ReplayName: "b", EventSourceArn: "x", State: "RUNNING" }])).toBe("2 replays: 1 completed and 1 running.");
    const d = { Name: "finance-webhook", ApiDestinationArn: "x", ConnectionArn: `${A}:connection/finance-api/2f8c9a31-0000-0000-0000-000000000000`, InvocationEndpoint: "https://x", HttpMethod: "POST", ApiDestinationState: "ACTIVE" };
    expect(destsSentence([d, { ...d, Name: "ops" }])).toBe("2 API destinations, both active. finance-webhook signs in through the finance-api connection.");
  });
});

import { busesSentence } from "./words";
describe("the line over the buses", () => {
  test("what each holds", () => {
    const rules = { larder: [{ Name: "a" }, { Name: "b" }], default: [{ Name: "nightly-export", ScheduleExpression: "rate(1 day)" }], "larder-audit": [] };
    const archives = [1, 2, 3].map((n) => ({ ArchiveName: `a${n}`, EventSourceArn: `${A}:event-bus/larder` }));
    expect(busesSentence(["default", "larder", "larder-audit"], rules, archives)).toBe("3 event buses. default runs the schedule rule nightly-export; larder has 2 rules and 3 archives; larder-audit has no rules yet.");
    expect(busesSentence([], {}, [])).toBe("");
  });
});

describe("a pattern as the boards print it", () => {
  test("objects over lines, lists on one", () => {
    expect(prettyPattern('{"source":["larder.orders"],"detail":{"total_cents":[{"numeric":[">",0]}]}}')).toBe('{\n  "source": ["larder.orders"],\n  "detail": {\n    "total_cents": [{ "numeric": [">", 0] }]\n  }\n}');
    expect(prettyPattern("nope")).toBe("nope");
  });
});

describe("what a rule's delete leaves alone", () => {
  test("names a topic with its subscriptions, as the board does", async () => {
    const { untouchedWords } = await import("./words");
    expect(untouchedWords(["arn:aws:sns:us-east-1:123456789012:orders-created"])).toBe("The orders-created topic and its subscriptions are not touched.");
    expect(untouchedWords(["arn:aws:sqs:us-east-1:123456789012:pick-lists"])).toBe("The pick-lists queue is not touched.");
    expect(untouchedWords(["arn:aws:sqs:us-east-1:123456789012:pick-lists", "arn:aws:lambda:us-east-1:123456789012:function:send-email"])).toBe("The pick-lists queue and the send-email function are not touched.");
    expect(untouchedWords([])).toBeNull();
  });
});
