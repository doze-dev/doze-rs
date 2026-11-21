import { describe, expect, test } from "bun:test";
import { aliasProblems, batchMost, destinationProblems, envInput, envProblems, generalChange, generalOf, generalProblems, mappingUpdate, nextVersion, permissionInput, permissionProblems, urlInput } from "./forms";
import { type Fn, chipWords, policyTrigger, runtimeName, sentenceOf, sourceWords, statementsOf, tailOf, triggersOf, urlAuthNote, wiredOf } from "./model";
import { firstEvent, templatesFor } from "./templates";

const fn: Fn = { name: "send-email", arn: "arn:aws:lambda:us-east-1:1:function:send-email", description: "Send alert", runtime: "python3.13", handler: "lambda_function.lambda_handler", memory: 256, timeout: 30, ephemeral: 512, role: "arn:aws:iam::1:role/lambda-exec", roleName: "lambda-exec", state: "Active", pkg: "Zip", arch: "x86_64", layers: [], env: {}, reserved: null, url: null, urlAuth: null, cors: null, lastModified: "" };

const POLICY = JSON.stringify({ Version: "2012-10-17", Statement: [{ Sid: "sns-alerts", Effect: "Allow", Principal: { Service: "sns.amazonaws.com" }, Action: "lambda:InvokeFunction", Resource: "x", Condition: { ArnLike: { "AWS:SourceArn": "arn:aws:sns:us-east-1:1:alerts" } } }] });

describe("what a function is called and what triggers it", () => {
  test("runtimes in the console's words", () => {
    expect(runtimeName("python3.13")).toBe("Python 3.13");
    expect(runtimeName("nodejs22.x")).toBe("Node.js 22.x");
    expect(runtimeName("ruby3.2")).toBe("Ruby 3.2");
    expect(runtimeName("provided.al2023")).toBe("Amazon Linux 2023");
  });
  test("names come from ARNs", () => {
    expect(tailOf("arn:aws:iam::1:role/path/lambda-exec")).toBe("lambda-exec");
    expect(tailOf("arn:aws:sqs:us-east-1:1:email-outbox")).toBe("email-outbox");
    expect(sourceWords("arn:aws:sqs:us-east-1:1:email-outbox").words).toBe("SQS queue email-outbox");
  });
  test("a policy statement from SNS is a trigger", () => {
    const [s] = statementsOf(POLICY);
    expect(policyTrigger(s!)?.words).toBe("SNS topic alerts");
    expect(statementsOf(undefined)).toEqual([]);
    expect(statementsOf("not json")).toEqual([]);
  });
  test("the sentence says what runs it, what triggers it and when it last ran", () => {
    const t = triggersOf([{ name: "u1", cells: ["email-outbox", "Enabled", "10", "u1", "arn:aws:sqs:us-east-1:1:email-outbox"] }], statementsOf(POLICY));
    expect(t.map((x) => x.words)).toEqual(["SNS topic alerts", "SQS queue email-outbox"]);
    expect(sentenceOf(fn, t, "08:13:58")).toBe("send-email runs Python 3.13 with 256 MB and a 30 second timeout. SNS topic alerts and SQS queue email-outbox trigger it; it last ran at 08:13:58.");
    expect(sentenceOf(fn, [], null)).toContain("Nothing triggers it yet.");
    expect(sentenceOf(fn, t.slice(0, 1), null)).toContain("SNS topic alerts triggers it.");
    expect(wiredOf(fn, t).map((w) => chipWords(w, t))).toEqual(["SNS topic alerts", "SQS queue email-outbox", "role lambda-exec"]);
  });
});

describe("the forms' rules and what they send", () => {
  test("a general edit sends only what changed", () => {
    const g = { ...generalOf(fn), memory: "512" };
    expect(generalChange(fn, g)).toEqual({ MemorySize: 512 });
    expect(generalProblems(g)).toEqual({});
    expect(generalProblems({ ...g, memory: "64", timeout: "901", ephemeral: "100" })).toHaveProperty("memory");
    expect(generalChange(fn, { ...g, ephemeral: "1024", role: "arn:aws:iam::1:role/other" })).toEqual({ MemorySize: 512, EphemeralStorage: { Size: 1024 }, Role: "arn:aws:iam::1:role/other" });
  });
  test("environment variables: key rule, reserved keys, 4 KB, the whole map", () => {
    expect(envProblems([{ key: "RETRY_LIMIT", value: "3" }])).toEqual({});
    expect(envProblems([{ key: "1bad", value: "" }])["0"]).toContain("starts with a letter");
    expect(envProblems([{ key: "AWS_REGION", value: "x" }])["0"]).toContain("sets itself");
    expect(envProblems([{ key: "BIG", value: "x".repeat(4100) }])[""]).toContain("4 KB");
    expect(envInput([{ key: "A_B", value: "1" }, { key: "", value: "" }])).toEqual({ Environment: { Variables: { A_B: "1" } } });
  });
  test("triggers, destinations, permissions, urls and aliases", () => {
    expect(batchMost("arn:aws:sqs:us-east-1:1:q.fifo")).toBe(10);
    expect(batchMost("arn:aws:sqs:us-east-1:1:q")).toBe(10000);
    expect(mappingUpdate("u1", { enabled: true, batch: 10 }, { queue: null, enabled: true, batch: "5" })).toEqual({ UUID: "u1", BatchSize: 5 });
    expect(destinationProblems({ condition: "OnFailure", type: "sqs", target: "arn:aws:sqs:us-east-1:1:orders.fifo" }).target).toContain("FIFO");
    expect(permissionProblems({ sid: "sns-alerts", principal: "sns.amazonaws.com", sourceArn: "", sourceAccount: "", action: "lambda:InvokeFunction" }, ["sns-alerts"]).sid).toContain("taken");
    expect(permissionInput("f", { sid: "s", principal: " s3.amazonaws.com ", sourceArn: "arn:aws:s3:::r", sourceAccount: "", action: "lambda:InvokeFunction" })).toEqual({ FunctionName: "f", StatementId: "s", Action: "lambda:InvokeFunction", Principal: "s3.amazonaws.com", SourceArn: "arn:aws:s3:::r" });
    expect(urlInput("f", { auth: "NONE", origins: "https://a.dev, https://b.dev", methods: "POST", headers: "", maxAge: "5" })).toEqual({ FunctionName: "f", AuthType: "NONE", Cors: { AllowOrigins: ["https://a.dev", "https://b.dev"], AllowMethods: ["POST"], MaxAge: 5 } });
    expect(urlInput("f", { auth: "AWS_IAM", origins: "", methods: "", headers: "", maxAge: "" })).toEqual({ FunctionName: "f", AuthType: "AWS_IAM" });
    expect(aliasProblems({ name: "123", description: "", version: "1" }, []).name).toContain("not only digits");
    expect(aliasProblems({ name: "live", description: "", version: "1" }, ["live"]).name).toContain("already exists");
    expect(nextVersion([{ Version: "$LATEST" }, { Version: "3" }, { Version: "2" }])).toBe(4);
  });
  test("example events carry the function's own triggers", () => {
    const t = triggersOf([], statementsOf(POLICY));
    const templates = templatesFor(t, { region: "us-east-1", account: "1" });
    expect(templates[0]?.label).toBe("SNS topic alerts");
    expect(firstEvent(templates).text).toContain("arn:aws:sns:us-east-1:1:alerts");
    expect(firstEvent(templatesFor([], { region: "r", account: "a" })).id).toBe("hello");
  });
});

describe("what the URL's auth type means here", () => {
  test("an AWS_IAM URL is not served unsigned: it wants a signature and answers 403 without one", () => {
    expect(urlAuthNote("AWS_IAM")).toContain("needs a SigV4 signature");
    expect(urlAuthNote("AWS_IAM")).toContain("403 Forbidden");
    expect(urlAuthNote("AWS_IAM")).not.toContain("unsigned");
    expect(urlAuthNote(null)).toBe(urlAuthNote("AWS_IAM"));
  });
  test("a NONE URL answers anyone the policy allows", () => {
    expect(urlAuthNote("NONE")).toContain("resource-based policy");
  });
});
