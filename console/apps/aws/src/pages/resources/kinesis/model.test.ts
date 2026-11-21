import { describe, expect, test } from "bun:test";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { createInput, dataText, daysWords, emptyCreate, estimate, iteratorInput, listSentence, metricsDiff, nameProblem, noPolicy, principalProblem, retentionOp, retentionProblem, shardsProblem, sharePolicy, shortSequence, statementsOf, streamRows, streamSentence } from "./model";

const row = (name: string, mode: string, consumers: string): ResourceRow => ({ name, label: name, id: name, cells: ["ACTIVE", mode, "1794000000000", consumers], link: null });

describe("the list's sentence", () => {
  const rows = streamRows([row("clickstream", "PROVISIONED", "1"), row("orders-feed", "ON_DEMAND", "0"), row("inventory-feed", "PROVISIONED", "0")]);
  test("counts the modes and names the consumers", () => {
    expect(listSentence(rows, { clickstream: ["search-indexer"] })).toBe("3 data streams: 2 provisioned and 1 on-demand. clickstream has 1 registered consumer, search-indexer.");
  });
  test("says the count of consumers while their names are not read", () => expect(listSentence(rows, {})).toContain("clickstream has 1 registered consumer."));
  test("none says nothing", () => expect(listSentence([], {})).toBe(""));
});

describe("a stream's sentence", () => {
  const s = { name: "clickstream", arn: "", status: "ACTIVE", mode: "PROVISIONED", shards: 2, consumers: 1, retention: 24, encryption: null, kms: null, created: null };
  test("names its consumer and what it reads with", () => expect(streamSentence(s, ["search-indexer"])).toBe("clickstream has 2 open shards, keeps records for 24 hours, and has 1 registered consumer, search-indexer, which reads it with GetRecords."));
  test("a stream with none says so", () => expect(streamSentence({ ...s, consumers: 0 }, [])).toBe("clickstream has 2 open shards, keeps records for 24 hours, and has no registered consumer."));
});

describe("the shard estimator", () => {
  test("is the board's figure", () => expect(estimate(2, 3000, 2)).toEqual({ shards: 6, formula: "max(6,000 / 1,024, 12,000 / 2,048) = 5.86, rounded up" }));
  test("rounds the record size up and never gives under one shard", () => {
    expect(estimate(1.2, 100, 0)?.shards).toBe(1);
    expect(estimate(1.2, 1024, 0)?.formula).toBe("max(2,048 / 1,024, 0 / 2,048) = 2.00, rounded up");
  });
  test("needs the three figures", () => {
    expect(estimate(0, 100, 1)).toBeNull();
    expect(estimate(Number.NaN, 100, 1)).toBeNull();
  });
});

describe("the rules of the fields", () => {
  test("a stream's name", () => {
    expect(nameProblem("orders-feed")).toBeNull();
    expect(nameProblem("")).toBe("Enter a name.");
    expect(nameProblem("bad name")).not.toBeNull();
    expect(nameProblem("x".repeat(129))).toContain("128");
  });
  test("shards and retention", () => {
    expect(shardsProblem("6")).toBeNull();
    expect(shardsProblem("0")).not.toBeNull();
    expect(shardsProblem("501")).not.toBeNull();
    expect(retentionProblem("72")).toBeNull();
    expect(retentionProblem("23")).not.toBeNull();
    expect(retentionProblem("8761")).not.toBeNull();
  });
  test("the retention in days and the call that gets there", () => {
    expect(daysWords(72)).toBe("3 days");
    expect(daysWords(36)).toBe("1 day 12 hours");
    expect(daysWords(12)).toBe("");
    expect(retentionOp(24, 72)).toBe("IncreaseStreamRetentionPeriod");
    expect(retentionOp(72, 24)).toBe("DecreaseStreamRetentionPeriod");
    expect(retentionOp(24, 24)).toBeNull();
  });
  test("the metrics to enable and to disable", () => expect(metricsDiff(["A", "B"], ["B", "C"])).toEqual({ enable: ["C"], disable: ["A"] }));
});

describe("the requests", () => {
  test("an on-demand create has no shard count; a provisioned one has", () => {
    expect(createInput({ ...emptyCreate, name: "s" })).toEqual({ StreamName: "s", StreamModeDetails: { StreamMode: "ON_DEMAND" } });
    expect(createInput({ name: "s", mode: "PROVISIONED", shards: "6", tags: [{ key: "stack", value: "larder" }, { key: "", value: "x" }] })).toEqual({ StreamName: "s", ShardCount: 6, StreamModeDetails: { StreamMode: "PROVISIONED" }, Tags: { stack: "larder" } });
  });
  test("an iterator names its starting point", () => {
    expect(iteratorInput("shardId-0", "TRIM_HORIZON", { sequence: "", at: null })).toEqual({ ShardId: "shardId-0", ShardIteratorType: "TRIM_HORIZON" });
    expect(iteratorInput("shardId-0", "AT_SEQUENCE_NUMBER", { sequence: " 49 ", at: null })).toEqual({ ShardId: "shardId-0", ShardIteratorType: "AT_SEQUENCE_NUMBER", StartingSequenceNumber: "49" });
    expect(iteratorInput("shardId-0", "AT_TIMESTAMP", { sequence: "", at: 5000 })).toEqual({ ShardId: "shardId-0", ShardIteratorType: "AT_TIMESTAMP", Timestamp: 5 });
  });
});

describe("a record", () => {
  test("a long sequence number is cut to its ends", () => expect(shortSequence("49668410271823404910264975263410584201823049381274698754")).toBe("49668410…698754"));
  test("data is base64 text, or not text", () => {
    expect(dataText(btoa('{"a":1}'))).toBe('{"a":1}');
    expect(dataText(btoa(String.fromCharCode(0xff, 0xfe)))).toBeNull();
    expect(dataText(undefined)).toBe("");
  });
});

describe("the sharing policy", () => {
  const arn = "arn:aws:kinesis:us-east-1:123456789012:stream/clickstream";
  test("is the guide's read-access shape", () => {
    const p = JSON.parse(sharePolicy("arn:aws:iam::123456789012:role/report-job", ["kinesis:GetRecords"], arn)) as { Version: string; Statement: { Effect: string; Principal: { AWS: string }; Action: string[]; Resource: string }[] };
    expect(p.Version).toBe("2012-10-17");
    expect(p.Statement[0]).toMatchObject({ Effect: "Allow", Principal: { AWS: "arn:aws:iam::123456789012:role/report-job" }, Action: ["kinesis:GetRecords"], Resource: arn });
  });
  test("a principal is an account, a user or a role", () => {
    expect(principalProblem("123456789012")).toBeNull();
    expect(principalProblem("arn:aws:iam::123456789012:role/report-job")).toBeNull();
    expect(principalProblem("report-job")).not.toBeNull();
  });
  test("its statements are read back, and no policy is the empty one", () => {
    expect(statementsOf(sharePolicy("123456789012", ["kinesis:ListShards"], arn))).toEqual([{ sid: "sharedthroughputreadstatement", effect: "Allow", principals: ["123456789012"], actions: ["kinesis:ListShards"] }]);
    expect(statementsOf("not json")).toBeNull();
    expect(noPolicy("{}")).toBe(true);
    expect(noPolicy(undefined)).toBe(true);
    expect(noPolicy("{\"Version\":\"2012-10-17\"}")).toBe(false);
  });
});
