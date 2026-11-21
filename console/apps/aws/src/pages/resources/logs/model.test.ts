import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { activityWords, createInput, filterNameProblem, groupNameProblem, groupRows, isOwnGroup, kmsProblem, listSentence, messagesOf, metricInput, metricNameProblem, namespaceProblem, retentionWords, valueProblem } from "./model";

const row = (name: string, cls: string, days: string | null, filters: string): ResourceRow => ({ name, label: name, id: name, cells: [cls, days, filters, "1794000000000"], link: null });

describe("the list's sentence", () => {
  const rows = groupRows([row("/aws/lambda/a", "STANDARD", null, "0"), row("/larder/api", "STANDARD", "30", "2"), row("/larder/scratch", "STANDARD", null, "0")]);
  test("names the group with filters, its retention and where to change them", () => {
    expect(listSentence(rows)).toBe("3 log groups. /larder/api has 2 metric filters and keeps events for 30 days. Choose a retention in the Retention column to change it, or the number in the Metric filters column to see a group's filters.");
  });
  test("with no filters it names a group that has a retention, or says none has", () => {
    expect(listSentence(groupRows([row("/a", "STANDARD", "7", "0")]))).toBe("1 log group. /a keeps events for 7 days. Choose a retention in the Retention column to change it.");
    expect(listSentence(groupRows([row("/a", "STANDARD", null, "0")]))).toContain("None has a retention policy.");
  });
  test("says nothing of no groups", () => expect(listSentence([])).toBe(""));
  test("reads a missing retention as Never Expire", () => {
    expect(rows[0]?.retention).toBeNull();
    expect(retentionWords(null)).toBe("Never Expire");
    expect(retentionWords(1)).toBe("1 day");
    expect(retentionWords(30)).toBe("30 days");
  });
});

describe("a group's activity", () => {
  test("counts the last hour and says where the newest event is", () => {
    expect(activityWords("/larder/api", { events: 1204, streams: 5, more: false }, { at: "08:14:03", stream: "payments-8c2e1" }, 5)).toBe("/larder/api took 1,204 events in the last hour from 5 streams. The newest event arrived at 08:14:03 in payments-8c2e1.");
  });
  test("marks a count that was cut", () => expect(activityWords("/g", { events: 10000, streams: 2, more: true }, null, 2)).toContain("10,000+ events"));
  test("an empty group says nothing has written to it", () => expect(activityWords("/g", null, null, 0)).toBe("/g is empty. Nothing has written to it yet."));
  test("a quiet hour is said as it is", () => expect(activityWords("/g", { events: 0, streams: 0, more: false }, { at: "01:00:00", stream: "s" }, 1)).toBe("/g took no events in the last hour. The newest event arrived at 01:00:00 in s."));
});

describe("the rules of the fields", () => {
  test("a group's name", () => {
    expect(groupNameProblem("/larder/notifications")).toBeNull();
    expect(groupNameProblem("")).toBe("Enter a name.");
    expect(groupNameProblem("aws/own")).toContain("aws/");
    expect(groupNameProblem("has space")).toContain("letters");
    expect(groupNameProblem("x".repeat(513))).toContain("512");
  });
  test("a key's ARN", () => {
    expect(kmsProblem("")).toBeNull();
    expect(kmsProblem("arn:aws:kms:us-east-1:123456789012:key/abc-123")).toBeNull();
    expect(kmsProblem("alias/larder")).not.toBeNull();
  });
  test("a filter's, a metric's and a namespace's names", () => {
    expect(filterNameProblem("api-errors")).toBeNull();
    expect(filterNameProblem("a:b")).not.toBeNull();
    expect(metricNameProblem("Api$Errors")).not.toBeNull();
    expect(metricNameProblem("ApiErrors")).toBeNull();
    expect(namespaceProblem("Larder/Api")).toBeNull();
    expect(namespaceProblem("Bad Space")).not.toBeNull();
  });
  test("a metric value is a number or a field reference", () => {
    expect(valueProblem("1")).toBeNull();
    expect(valueProblem("$.latency")).toBeNull();
    expect(valueProblem("one")).not.toBeNull();
  });
  test("a function subscribing to its own group", () => {
    expect(isOwnGroup("/aws/lambda/send-email", "send-email")).toBe(true);
    expect(isOwnGroup("/larder/api", "send-email")).toBe(false);
  });
});

describe("the requests", () => {
  test("a create sends the class, and the key and the protection only when asked", () => {
    expect(createInput({ name: "/g", cls: "STANDARD", kms: "", protect: false })).toEqual({ logGroupName: "/g", logGroupClass: "STANDARD" });
    expect(createInput({ name: "/g", cls: "INFREQUENT_ACCESS", kms: " arn:k ", protect: true })).toEqual({ logGroupName: "/g", logGroupClass: "INFREQUENT_ACCESS", kmsKeyId: "arn:k", deletionProtectionEnabled: true });
  });
  test("a metric filter is one transformation, the default value and the unit when set", () => {
    const f = { name: "n", namespace: "Larder/P", metric: "M", value: "1", fallback: "", unit: "" };
    expect(metricInput("/g", "ERROR", f)).toEqual({ logGroupName: "/g", filterName: "n", filterPattern: "ERROR", metricTransformations: [{ metricNamespace: "Larder/P", metricName: "M", metricValue: "1" }] });
    expect(metricInput("/g", "", { ...f, fallback: "0", unit: "Count" }).metricTransformations).toEqual([{ metricNamespace: "Larder/P", metricName: "M", metricValue: "1", defaultValue: 0, unit: "Count" }]);
    expect(metricInput("/g", "", { ...f, unit: "None" }).metricTransformations).toEqual([{ metricNamespace: "Larder/P", metricName: "M", metricValue: "1" }]);
  });
  test("test messages are the non-empty lines", () => expect(messagesOf("a\n\n  b  \n")).toEqual(["a", "b"]));
});
