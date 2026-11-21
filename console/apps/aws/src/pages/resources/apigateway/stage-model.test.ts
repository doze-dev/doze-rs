import { describe, expect, test } from "bun:test";
import { detailsOf, detailsPatches, levelWords, loggingOf, loggingPatches, variablesPatches } from "./stage-model";
import type { Stage } from "./types";

const prod: Stage = { stageName: "prod", description: "Production", methodSettings: { "*/*": { loggingLevel: "ERROR", dataTraceEnabled: false, metricsEnabled: false, throttlingRateLimit: 10000, throttlingBurstLimit: 5000 } }, accessLogSettings: { destinationArn: "arn:aws:logs:us-east-1:1:log-group:/larder/api", format: "$context.requestId" } };

describe("a stage's details", () => {
  test("throttling is off at the account's defaults", () => {
    expect(detailsOf(prod)).toEqual({ description: "Production", cache: false, throttle: false, rate: "10000", burst: "5000" });
  });
  test("an edit that changes nothing sends nothing", () => {
    expect(detailsPatches(prod, detailsOf(prod))).toEqual([]);
  });
  test("turning throttling on sends the rate and the burst, and a new description", () => {
    expect(detailsPatches(prod, { ...detailsOf(prod), description: "Live", throttle: true, rate: "50", burst: "100" })).toEqual([
      { op: "replace", path: "/description", value: "Live" },
      { op: "replace", path: "/*/*/throttling/rateLimit", value: "50" },
      { op: "replace", path: "/*/*/throttling/burstLimit", value: "100" },
    ]);
  });
});

describe("logging and tracing", () => {
  test("reads the level and the access log; turning the access log off removes it", () => {
    expect(loggingOf(prod)).toMatchObject({ level: "ERROR", access: true });
    expect(levelWords("ERROR")).toBe("Errors only");
    expect(loggingPatches(prod, { ...loggingOf(prod), access: false })).toEqual([{ op: "remove", path: "/accessLogSettings" }]);
    expect(loggingPatches(prod, { ...loggingOf(prod), level: "INFO", dataTrace: true })).toEqual([
      { op: "replace", path: "/*/*/logging/loglevel", value: "INFO" },
      { op: "replace", path: "/*/*/logging/dataTrace", value: "true" },
    ]);
  });
});

describe("stage variables", () => {
  test("a changed or new one is replaced, one that is gone is removed", () => {
    expect(variablesPatches({ a: "1", b: "2" }, [{ name: "a", value: "1" }, { name: "c", value: "3" }])).toEqual([
      { op: "replace", path: "/variables/c", value: "3" },
      { op: "remove", path: "/variables/b" },
    ]);
  });
});
